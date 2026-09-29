//! The installer.
//!
//! # What it is
//!
//! One file a person downloads and runs. It is a small program with the
//! program it installs carried at its end: the files as a ZIP archive, and
//! after them sixteen bytes saying where the archive starts and that it is
//! there at all. That is how installers have been made since there were
//! installers — a program does not mind what follows it in its own file,
//! and the system that starts it reads only its own headers — and it means
//! the installer is made by putting files after a program rather than by
//! building anything: [`pack`].
//!
//! # What it does
//!
//! Puts the files where the desktop keeps programs a person installed for
//! themselves ([`wp_shell::install::folder`]), leaves a copy of itself
//! beside them without the files to take them off again, and tells the
//! desktop: [`wp_shell::install::register`]. Nothing asks for an
//! administrator, and a second run over the first replaces the files and
//! says the same things again.
//!
//! Taking it off is the reverse: [`uninstall`] removes the files the
//! installer wrote, which it wrote down as it wrote them, takes back what
//! the desktop was told, and removes the folder if nothing else is in it.

#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Path, PathBuf};

pub use wp_shell::install::{Installed, PROGRAM_NAME, QUIET_SWITCH, UNINSTALL_SWITCH};

/// What the last eight bytes of an installer are.
pub const MAGIC: &[u8; 8] = b"WPSETUP1";

/// The windowed program, which is the one the kinds open with.
pub fn program_file() -> String {
    format!("word-processor{}", std::env::consts::EXE_SUFFIX)
}

/// The copy of the installer left behind to take the program off.
pub fn uninstaller_file() -> String {
    format!("uninstall{}", std::env::consts::EXE_SUFFIX)
}

/// The list of the files the installer wrote, one to a line, which is what
/// the uninstaller removes and nothing else.
pub const RECORD: &str = "installed.txt";

/// Why an installer could not do what it was asked.
#[derive(Debug)]
pub enum Failure {
    /// This is the installer without the files: nothing to install.
    NothingCarried,
    /// The files it carries cannot be read: the download is damaged.
    Damaged(String),
    /// What it carries has no program in it.
    NoProgram,
    /// The desktop does not say where a person's programs go.
    NoFolder,
    /// A file is in use — the program is running — and cannot be replaced
    /// or removed.
    InUse(PathBuf),
    /// Anything else the file system said.
    Io(PathBuf, std::io::Error),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingCarried => f.write_str("This installer carries no program to install."),
            Self::Damaged(why) => {
                write!(
                    f,
                    "This installer is damaged and cannot be used ({why}). Download it again."
                )
            }
            Self::NoProgram => f.write_str("This installer does not carry the program."),
            Self::NoFolder => {
                f.write_str("This computer does not say where programs are installed.")
            }
            Self::InUse(path) => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                write!(f, "{name} is in use. Close {PROGRAM_NAME} and try again.")
            }
            Self::Io(path, error) => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for Failure {}

/// An error the file system gave, as a failure: a file in use said as such,
/// because that is the one a person can do something about.
fn failed(path: &Path, error: std::io::Error) -> Failure {
    // Windows will not let a running program's file be replaced or removed:
    // it says access is denied, or that the file is shared. Linux lets both
    // be done and says "text file busy" only to writing over it in place,
    // which nothing here does.
    let in_use = if cfg!(windows) {
        matches!(error.raw_os_error(), Some(5 | 32))
    } else {
        error.raw_os_error() == Some(26)
    };
    if in_use {
        Failure::InUse(path.to_owned())
    } else {
        Failure::Io(path.to_owned(), error)
    }
}

/// Where the archive an installer carries starts, and the archive; `None`
/// for a program that carries nothing.
#[must_use]
pub fn payload(installer: &[u8]) -> Option<(usize, &[u8])> {
    let length = installer.len();
    if length < 16 || &installer[length - 8..] != MAGIC {
        return None;
    }
    let mut start = [0u8; 8];
    start.copy_from_slice(&installer[length - 16..length - 8]);
    let start = usize::try_from(u64::from_le_bytes(start)).ok()?;
    (start <= length - 16).then(|| (start, &installer[start..length - 16]))
}

/// The installer without what it carries: the program that installs, which
/// is also what takes the program off again.
#[must_use]
pub fn stub(installer: &[u8]) -> &[u8] {
    payload(installer).map_or(installer, |(start, _)| &installer[..start])
}

/// An installer: the program that installs, and after it the files it is to
/// install. A program that already carries files has them replaced.
///
/// # Errors
/// Where a name cannot stand in the archive.
pub fn pack(installer: &[u8], files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut archive = wp_zip::ZipWriter::new();
    for (name, data) in files {
        archive.add(name, data).map_err(|error| format!("{name}: {error}"))?;
    }
    let archive = archive.finish().map_err(|error| error.to_string())?;
    let mut out = stub(installer).to_vec();
    let start = out.len() as u64;
    out.extend_from_slice(&archive);
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(MAGIC);
    Ok(out)
}

/// The files an installer carries, each by its name.
///
/// # Errors
/// Where it carries none, or they cannot be read.
pub fn carried(installer: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let (_, archive) = payload(installer).ok_or(Failure::NothingCarried)?;
    let archive =
        wp_zip::ZipArchive::open(archive).map_err(|error| Failure::Damaged(error.to_string()))?;
    let mut files = Vec::new();
    for entry in archive.entries() {
        // A file goes into the folder and nowhere else: a name with a folder
        // in it is not one this installer wrote.
        if entry.name.contains(['/', '\\']) || entry.name.starts_with('.') {
            return Err(Failure::Damaged(format!("{} is not a file name", entry.name)));
        }
        let data = archive.read(entry).map_err(|error| Failure::Damaged(error.to_string()))?;
        files.push((entry.name.clone(), data));
    }
    Ok(files)
}

/// What an installed program is, from the folder it is in.
#[must_use]
pub fn installed_in(folder: &Path) -> Installed {
    let size = recorded(folder)
        .iter()
        .chain(std::iter::once(&uninstaller_file()))
        .filter_map(|name| std::fs::metadata(folder.join(name)).ok())
        .map(|metadata| metadata.len())
        .sum();
    Installed {
        folder: folder.to_owned(),
        program: folder.join(program_file()),
        uninstaller: folder.join(uninstaller_file()),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        size,
    }
}

/// The files the installer wrote down as it wrote them.
fn recorded(folder: &Path) -> Vec<String> {
    std::fs::read_to_string(folder.join(RECORD))
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|name| !name.is_empty() && !name.contains(['/', '\\']))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Writes a file whole or not at all: to a name beside it first, then over
/// it. A program half written is worse than the old one left in place.
fn write_whole(path: &Path, data: &[u8], runnable: bool) -> Result<(), Failure> {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".new");
    let beside = PathBuf::from(beside);
    std::fs::write(&beside, data).map_err(|error| failed(&beside, error))?;
    #[cfg(unix)]
    if runnable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&beside, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| failed(&beside, error))?;
    }
    #[cfg(not(unix))]
    let _ = runnable;
    std::fs::rename(&beside, path).map_err(|error| {
        let _ = std::fs::remove_file(&beside);
        failed(path, error)
    })
}

/// Installs what the installer carries into the folder and tells the
/// desktop. Returns what was installed and whether the desktop took all of
/// it; the files are in place either way.
///
/// # Errors
/// Where there is nothing to install, or a file cannot be written.
pub fn install(installer: &[u8], folder: &Path) -> Result<(Installed, bool), Failure> {
    let files = carried(installer)?;
    let program = program_file();
    if !files.iter().any(|(name, _)| *name == program) {
        return Err(Failure::NoProgram);
    }
    std::fs::create_dir_all(folder).map_err(|error| failed(folder, error))?;
    // The program first: it is the one that is running if any is, and then
    // nothing else is touched.
    let mut order: Vec<&(String, Vec<u8>)> = files.iter().collect();
    order.sort_by_key(|(name, _)| *name != program);
    let mut record = String::new();
    for (name, data) in order {
        write_whole(&folder.join(name), data, true)?;
        record.push_str(name);
        record.push('\n');
    }
    write_whole(&folder.join(uninstaller_file()), stub(installer), true)?;
    write_whole(&folder.join(RECORD), record.as_bytes(), false)?;
    let installed = installed_in(folder);
    let registered = wp_shell::install::register(&installed);
    Ok((installed, registered))
}

/// Takes the program in the folder off: its files, what the desktop was
/// told, and the folder if nothing else is left in it. The uninstaller that
/// is running cannot remove itself on Windows; it is left, and said so, for
/// the caller to see to once it has stopped.
///
/// Returns whether the desktop took back all it was told.
///
/// # Errors
/// Where the program is running, or a file cannot be removed.
pub fn uninstall(folder: &Path) -> Result<bool, Failure> {
    let installed = installed_in(folder);
    let program = program_file();
    // The program first: if it is running, nothing has been touched yet.
    let mut names = recorded(folder);
    names.sort_by_key(|name| *name != program);
    for name in &names {
        let path = folder.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failed(&path, error)),
        }
    }
    let unregistered = wp_shell::install::unregister(&installed);
    let _ = std::fs::remove_file(folder.join(RECORD));
    let running = std::env::current_exe().ok().and_then(|path| std::fs::canonicalize(path).ok());
    let uninstaller = std::fs::canonicalize(&installed.uninstaller).ok();
    if cfg!(not(windows)) || running.is_none() || running != uninstaller {
        let _ = std::fs::remove_file(&installed.uninstaller);
        // Only an empty folder is removed: whatever else is in it is not this
        // installer's to throw away.
        let _ = std::fs::remove_dir(folder);
    }
    Ok(unregistered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_packed_is_what_is_carried() {
        let installer =
            pack(b"MZ a program", &[("word-processor", b"the program"), ("wp", b"the other")])
                .expect("packed");
        assert!(installer.starts_with(b"MZ a program"), "the program is still where it was");
        assert!(installer.ends_with(MAGIC));
        assert_eq!(stub(&installer), b"MZ a program");
        let files = carried(&installer).expect("the files");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], ("word-processor".to_owned(), b"the program".to_vec()));
        assert_eq!(files[1], ("wp".to_owned(), b"the other".to_vec()));
    }

    #[test]
    fn packing_an_installer_again_replaces_what_it_carried() {
        let first = pack(b"stub", &[("a", b"one")]).expect("packed");
        let second = pack(&first, &[("b", b"two")]).expect("packed again");
        assert_eq!(stub(&second), b"stub");
        let files = carried(&second).expect("the files");
        assert_eq!(files, vec![("b".to_owned(), b"two".to_vec())]);
    }

    #[test]
    fn a_program_that_carries_nothing_says_so() {
        assert!(payload(b"just a program, long enough to have sixteen bytes").is_none());
        assert!(matches!(carried(b"short"), Err(Failure::NothingCarried)));
        assert_eq!(stub(b"just a program"), b"just a program");
    }

    #[test]
    fn a_damaged_download_is_found_before_anything_is_written() {
        let mut installer = pack(b"stub", &[("word-processor", &[7u8; 4000])]).expect("packed");
        // A byte of the compressed program changed — past the stub, the
        // entry's header of thirty bytes and its name — which the archive's
        // checksum is there to find.
        let at = 4 + 30 + "word-processor".len() + 2;
        installer[at] ^= 0xFF;
        match carried(&installer) {
            Err(Failure::Damaged(_)) => {}
            Err(error) => panic!("damaged, and said otherwise: {error}"),
            Ok(_) => panic!("a damaged program was taken as sound"),
        }
        // And a tail that points past the file.
        let mut pointing = pack(b"stub", &[("a", b"one")]).expect("packed");
        let length = pointing.len();
        pointing[length - 16..length - 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(payload(&pointing).is_none());
    }

    #[test]
    fn a_name_with_a_folder_in_it_is_refused() {
        let installer = pack(b"stub", &[("../evil", b"x")]);
        // The archive writer may refuse it itself; if it does not, reading
        // it back must.
        if let Ok(installer) = installer {
            assert!(matches!(carried(&installer), Err(Failure::Damaged(_))));
        }
    }
}
