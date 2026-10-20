//! Starting PerMark for the folders the menu was used on: the program with the folder as its one argument (or
//! `--new-markdown <folder>`), once per folder, as the installer version's entry does. Plain Rust, tested without a shell.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The program, as the package names it (`Executable` in the manifest).
pub const PROGRAM: &str = "permark.exe";

/// The most folders one use of the menu opens (Explorer's classic menu stops at 15 items too).
pub const MOST_FOLDERS: usize = 15;

/// The argument that makes a Markdown file in the folder and opens it.
pub const NEW_MARKDOWN_FLAG: &str = "--new-markdown";

/// The program of the package: beside this DLL.
#[must_use]
pub fn package_program(dll: &Path) -> Option<PathBuf> {
    Some(dll.parent()?.join(PROGRAM))
}

/// The folders to open: absolute paths only (one that is not, or starts like an option, is not given to the program),
/// each once (Windows paths ignore case), at most [`MOST_FOLDERS`], in order.
#[must_use]
pub fn folders_to_open(selected: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    selected
        .into_iter()
        .filter(|path| path.is_absolute())
        .filter(|path| seen.insert(path.to_string_lossy().to_lowercase()))
        .take(MOST_FOLDERS)
        .collect()
}

/// The command for one folder. The program starts in its own folder (a working folder cannot be renamed or deleted
/// meanwhile, and the person may want to do that to the very folder they opened), its streams closed.
#[must_use]
pub fn command(program: &Path, folder: &Path, new_markdown: bool) -> Command {
    let mut command = Command::new(program);
    if new_markdown {
        command.arg(NEW_MARKDOWN_FLAG);
    }
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

/// Starts `program` once per folder. Fine when at least one started; the first error when none did. Never waits.
///
/// # Errors
/// Nothing started (no folder, or every start failed).
pub fn start(program: &Path, folders: &[PathBuf], new_markdown: bool) -> io::Result<()> {
    let mut started: Vec<Child> = Vec::new();
    let mut first_error = None;
    for folder in folders {
        match command(program, folder, new_markdown).spawn() {
            Ok(child) => started.push(child),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    // the programs run on their own: their handles close here
    let any = !started.is_empty();
    drop(started);
    match (any, first_error) {
        (true, _) => Ok(()),
        (false, Some(error)) => Err(error),
        (false, None) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "there is no folder to open",
        )),
    }
}
