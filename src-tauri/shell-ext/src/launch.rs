//! Starting PerMark for the folders the menu was used on.
//!
//! The same as the entry of the installer version: the program, with the folder's path as its one argument. The program
//! opens that folder as a project, and a project that is open already is its own window, so each folder gets its own
//! start (the classic menu does that too: Explorer runs the command once for each selected folder).
//!
//! Plain Rust, with nothing of COM in it, so the choices are tested without a shell.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The program, by the name the package gives it (`Executable` in `AppxManifest.xml`).
pub(crate) const PROGRAM: &str = "permark.exe";

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

/// The argument that asks the program to make a Markdown file in the folder and open that folder.
pub(crate) const NEW_MARKDOWN_FLAG: &str = "--new-markdown";

/// The command that makes a new Markdown file in `folder` and opens it. The flag and the folder are the two arguments.
pub(crate) fn command_new_markdown(program: &Path, folder: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .arg(NEW_MARKDOWN_FLAG)
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(home) = program.parent() {
        command.current_dir(home);
    }
    command
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

/// Starts `program` once for each of `folders`, and returns what started and the first error of what did not.
fn start(
    program: &Path,
    folders: &[PathBuf],
    new_markdown: bool,
) -> (Vec<Child>, Option<io::Error>) {
    let mut started = Vec::new();
    let mut first_error = None;
    for folder in folders {
        let spawned = if new_markdown {
            command_new_markdown(program, folder).spawn()
        } else {
            command(program, folder).spawn()
        };
        match spawned {
            Ok(child) => started.push(child),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    (started, first_error)
}

/// Starts `program` once for each of `folders`, making a Markdown file in each. Same result as [`open`].
pub(crate) fn new_markdown(program: &Path, folders: &[PathBuf]) -> io::Result<()> {
    conclude(start(program, folders, true))
}

/// Starts `program` once for each of `folders`. Fine when it started for at least one; the first error when it did not
/// start at all. It does not wait for the program.
pub(crate) fn open(program: &Path, folders: &[PathBuf]) -> io::Result<()> {
    conclude(start(program, folders, false))
}

/// The programs run on their own: the handles of the ones that started are closed here, and nothing waits.
fn conclude((started, first_error): (Vec<Child>, Option<io::Error>)) -> io::Result<()> {
    let started_any = !started.is_empty();
    drop(started);
    match (started_any, first_error) {
        (false, Some(error)) => Err(error),
        (false, None) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "there is no folder to open",
        )),
        (true, _) => Ok(()),
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
                r"C:\Program Files\WindowsApps\EdelmarSchneider.PerMark_0.5.1.0_x64__abc\permark_shell.dll"
            )),
            Some(PathBuf::from(
                r"C:\Program Files\WindowsApps\EdelmarSchneider.PerMark_0.5.1.0_x64__abc\permark.exe"
            ))
        );
        assert_eq!(
            package_program(Path::new(r"D:\Layout\Zoë\permark_shell.dll")),
            Some(PathBuf::from(r"D:\Layout\Zoë\permark.exe"))
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
            let command = command(Path::new(r"C:\Pkg\permark.exe"), Path::new(folder));
            let args: Vec<&OsStr> = command.get_args().collect();
            assert_eq!(args, [OsStr::new(folder)], "{folder}");
        }
    }

    #[test]
    fn a_new_markdown_file_passes_the_flag_and_then_the_folder() {
        let command = command_new_markdown(
            Path::new(r"C:\Pkg\permark.exe"),
            Path::new(r"C:\My Projects\notes"),
        );
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(
            args,
            [
                OsStr::new("--new-markdown"),
                OsStr::new(r"C:\My Projects\notes")
            ]
        );
        assert_eq!(command.get_current_dir(), Some(Path::new(r"C:\Pkg")));
    }

    #[test]
    fn the_program_starts_in_its_own_folder_and_not_in_the_one_it_opens() {
        let program = Path::new(r"C:\Pkg\permark.exe");
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
        let error = open(Path::new(r"C:\Pkg\permark.exe"), &[]).expect_err("no folder");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);

        // a folder that exists, with no program in it
        let missing = std::env::temp_dir().join("permark-shell-test-no-such-program.exe");
        let error = open(&missing, &paths(&[r"C:\Projects"])).expect_err("no program");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_program_that_exists_is_started_once_for_each_folder() {
        // A program that exits at once and shows nothing stands in for PerMark: `where.exe` with an argument it takes as
        // a file name to look for.
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let stand_in = system.join("System32").join("where.exe");
        if !stand_in.exists() {
            return;
        }
        let folders = paths(&[r"C:\one", r"C:\two"]);
        let (started, error) = start(&stand_in, &folders, false);
        // wait for them, so that no process of the test outlives it
        let ended: Vec<_> = started
            .into_iter()
            .map(|mut child| child.wait().is_ok())
            .collect();
        assert!(error.is_none());
        assert_eq!(ended, [true, true]);
    }
}
