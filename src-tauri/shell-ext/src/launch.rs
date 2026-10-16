//! Starting TrailMark for the folders the menu was used on.
//!
//! The same as the entry of the installer version: the program, with the folder's path as its one argument. The program
//! opens that folder as a project, and a project that is open already is its own window, so each folder gets its own
//! start (the classic menu does that too: Explorer runs the command once for each selected folder).
//!
//! Plain Rust, with nothing of COM in it, so the choices are tested without a shell.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The program, by the name the package gives it (`Executable` in `AppxManifest.xml`).
pub(crate) const PROGRAM: &str = "trailmark.exe";

/// The most folders that one use of the menu opens. Explorer's own classic menu stops showing a command that runs once for
/// each item at 15 items, and a handful of windows from one click is already a lot.
pub(crate) const MOST_FOLDERS: usize = 15;

/// The program of the package: it sits in the same folder as the extension's DLL (the package's root). That is how the
/// path is found, so it is right wherever Windows put this version of the package, and in a copy of the package.
pub(crate) fn package_program(dll: &Path) -> Option<PathBuf> {
    Some(dll.parent()?.join(PROGRAM))
}

/// The folders to open, from the paths of the selected items: in order, each once (Windows paths do not tell `A` from `a`),
/// at most [`MOST_FOLDERS`]. A path that is not absolute is left out: the shell hands over complete paths, and one that is
/// not (or one that starts like an option) is not something to give the program.
pub(crate) fn folders_to_open(selected: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    selected
        .into_iter()
        .filter(|path| path.is_absolute())
        .filter(|path| seen.insert(path.to_string_lossy().to_lowercase()))
        .take(MOST_FOLDERS)
        .collect()
}

/// The command that opens `folder` in `program`. The folder's path is the one argument, as the entry of the installer
/// version passes `"%1"`; Rust puts the quotes around it that a path with spaces needs.
///
/// The program starts in its own folder, not in the one it opens: a program's working folder cannot be renamed or deleted
/// meanwhile, and the person may well want to do that to the folder from the very Explorer window the menu was in. It has no
/// window of the surrogate to talk to, so its standard streams are closed.
pub(crate) fn command(program: &Path, folder: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(home) = program.parent() {
        command.current_dir(home);
    }
    command
}

/// Starts `program` once for each of `folders`. Fine when it started for at least one; the first error when it did not
/// start at all. It does not wait for the program.
pub(crate) fn open(program: &Path, folders: &[PathBuf]) -> io::Result<()> {
    let mut first_error = None;
    let mut started = 0;
    for folder in folders {
        match command(program, folder).spawn() {
            // The program runs on its own; its handle is not needed.
            Ok(_child) => started += 1,
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match (started, first_error) {
        (0, Some(error)) => Err(error),
        (0, None) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "there is no folder to open",
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_program_is_beside_the_dll_wherever_the_package_is() {
        assert_eq!(
            package_program(Path::new(
                r"C:\Program Files\WindowsApps\EdelmarSchneider.TrailMark_0.5.1.0_x64__abc\trailmark_shell.dll"
            )),
            Some(PathBuf::from(
                r"C:\Program Files\WindowsApps\EdelmarSchneider.TrailMark_0.5.1.0_x64__abc\trailmark.exe"
            ))
        );
        assert_eq!(
            package_program(Path::new(r"D:\Layout\Zoë\trailmark_shell.dll")),
            Some(PathBuf::from(r"D:\Layout\Zoë\trailmark.exe"))
        );
        // a DLL with no folder above it (not a path Windows gives) has no program beside it
        assert_eq!(package_program(Path::new("")), None);
    }

    #[test]
    fn the_folder_is_the_one_argument_as_it_is() {
        for folder in [
            r"C:\Projects\notes",
            r"C:\My Projects\with spaces",
            r"C:\Users\Zoë\Dokumente\Über uns",
            r"D:\",
            r"\\server\share\docs",
            r"C:\Trailing space \",
        ] {
            let command = command(Path::new(r"C:\Pkg\trailmark.exe"), Path::new(folder));
            let args: Vec<&OsStr> = command.get_args().collect();
            assert_eq!(args, [OsStr::new(folder)], "{folder}");
        }
    }

    #[test]
    fn the_program_starts_in_its_own_folder_and_not_in_the_one_it_opens() {
        let program = Path::new(r"C:\Pkg\trailmark.exe");
        let command = command(program, Path::new(r"C:\Projects\notes"));
        assert_eq!(command.get_program(), program.as_os_str());
        assert_eq!(command.get_current_dir(), Some(Path::new(r"C:\Pkg")));
    }

    #[test]
    fn each_folder_is_opened_once_in_order() {
        assert_eq!(
            folders_to_open(paths(&[r"C:\b", r"C:\a", r"C:\B", r"c:\A", r"C:\c"])),
            paths(&[r"C:\b", r"C:\a", r"C:\c"])
        );
    }

    #[test]
    fn only_complete_paths_are_opened() {
        assert_eq!(
            folders_to_open(paths(&[
                "notes",
                r"..\notes",
                "-flag",
                "",
                r"C:\ok",
                r"\\server\share\ok"
            ])),
            paths(&[r"C:\ok", r"\\server\share\ok"])
        );
    }

    #[test]
    fn a_click_opens_a_few_folders_and_not_a_hundred() {
        let many: Vec<PathBuf> = (0..40)
            .map(|n| PathBuf::from(format!(r"C:\p{n}")))
            .collect();
        let opened = folders_to_open(many.clone());
        assert_eq!(opened.len(), MOST_FOLDERS);
        assert_eq!(opened, many[..MOST_FOLDERS]);
    }

    #[test]
    fn there_is_nothing_to_start_without_a_folder_or_a_program() {
        let error = open(Path::new(r"C:\Pkg\trailmark.exe"), &[]).expect_err("no folder");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);

        // a folder that exists, with no program in it
        let missing = std::env::temp_dir().join("trailmark-shell-test-no-such-program.exe");
        let error = open(&missing, &paths(&[r"C:\Projects"])).expect_err("no program");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_program_that_exists_is_started_once_for_each_folder() {
        // A program that exits at once and shows nothing stands in for TrailMark: `where.exe` with an argument it takes as
        // a file name to look for.
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let stand_in = system.join("System32").join("where.exe");
        if !stand_in.exists() {
            return;
        }
        let folders = paths(&[r"C:\one", r"C:\two"]);
        assert!(open(&stand_in, &folders).is_ok());
    }
}
