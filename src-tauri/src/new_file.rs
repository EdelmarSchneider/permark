//! A Markdown file made from the file manager.
//!
//! "New Markdown file with PerMark" starts the program as `permark --new-markdown <folder>`. The folder
//! opens as a project, and the new file is what the window shows (the same channel as opening a file inside
//! a project). The file is `untitled.md`, or `untitled-2.md` and so on when that name is taken. It starts
//! with a title, the same way a file made inside the app does.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::commands::initial_path_from;

/// The argument the file manager's entry passes before the folder.
pub const NEW_MARKDOWN_FLAG: &str = "--new-markdown";

/// The folder this program was started to open with a new Markdown file, when it was.
static LAUNCH_FOLDER: OnceLock<String> = OnceLock::new();

/// The folder a `--new-markdown` launch opens. Both the window placement and the first screen ask for it.
pub fn launch_folder() -> Option<String> {
    LAUNCH_FOLDER.get().cloned()
}

/// What starting with `--new-markdown` decided: the folder to open, and the file that was made in it.
pub struct Prepared {
    pub folder: String,
    /// Absent when the folder could not be written: the folder still opens.
    pub file: Option<String>,
}

/// Makes the file and remembers the folder, before anything asks what this program was started to show.
/// A file the environment already names is left as it is.
pub fn prepare() {
    let args: Vec<OsString> = std::env::args_os().collect();
    let cwd = std::env::current_dir().ok();
    let Some(ready) = prepare_in(&args, cwd.as_deref()) else {
        return;
    };
    if std::env::var_os("PERMARK_SHOW").is_none() {
        if let Some(file) = &ready.file {
            std::env::set_var("PERMARK_SHOW", file);
        }
    }
    let _ = LAUNCH_FOLDER.set(ready.folder);
}

/// Reads `--new-markdown <folder>` from `args` (the program itself first) and makes the file.
/// `None` when this is not that launch, or the folder is not one that can be opened.
pub fn prepare_in(args: &[OsString], cwd: Option<&Path>) -> Option<Prepared> {
    if args.get(1).is_none_or(|arg| arg != NEW_MARKDOWN_FLAG) {
        return None;
    }
    let raw = args.get(2)?.clone();
    let folder = initial_path_from(raw, cwd.map(Path::to_path_buf))?;
    let path = Path::new(&folder);
    if !path.is_dir() {
        return None;
    }
    let file = create_untitled_markdown(path)
        .ok()
        .map(|made| made.display().to_string());
    Some(Prepared { folder, file })
}

/// `untitled.md` in `folder`, or the next free `untitled-N.md`. The text is a title and a final newline.
pub fn create_untitled_markdown(folder: &Path) -> io::Result<PathBuf> {
    let name = fresh_name(folder);
    let path = folder.join(&name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let written = file.write_all(blank_markdown(&name).as_bytes());
    if written.is_err() {
        drop(file);
        let _ = std::fs::remove_file(&path);
    }
    written?;
    Ok(path)
}

/// The first of `untitled.md`, `untitled-2.md`... that is not already in `folder`.
fn fresh_name(folder: &Path) -> String {
    let taken: std::collections::HashSet<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .map(|name| name.to_lowercase())
        .collect();
    if !taken.contains("untitled.md") {
        return "untitled.md".to_owned();
    }
    for n in 2..10_000 {
        let name = format!("untitled-{n}.md");
        if !taken.contains(&name) {
            return name;
        }
    }
    "untitled.md".to_owned()
}

/// A title from the file name: dashes and underscores become spaces.
fn blank_markdown(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let title = stem.replace(['-', '_'], " ");
    let title = title.trim();
    let title = if title.is_empty() { stem } else { title };
    format!("# {title}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_launch_without_the_flag_makes_nothing() {
        let dir = tempfile::tempdir().expect("temp");
        assert!(prepare_in(&args(&["permark", &dir.path().display().to_string()]), None).is_none());
        assert!(prepare_in(&args(&["permark"]), None).is_none());
        assert!(prepare_in(&args(&["permark", "--notes"]), None).is_none());
    }

    #[test]
    fn the_flag_makes_untitled_and_opens_the_folder() {
        let dir = tempfile::tempdir().expect("temp");
        let folder = dir.path().display().to_string();
        let ready =
            prepare_in(&args(&["permark", NEW_MARKDOWN_FLAG, &folder]), None).expect("a launch");
        assert_eq!(
            ready.folder,
            dunce::canonicalize(dir.path())
                .expect("canonical")
                .display()
                .to_string()
        );
        let file = ready.file.expect("the file");
        assert!(file.to_lowercase().ends_with("untitled.md"));
        assert_eq!(
            std::fs::read_to_string(&file).expect("read"),
            "# untitled\n"
        );
    }

    #[test]
    fn a_taken_name_gets_the_next_number() {
        let dir = tempfile::tempdir().expect("temp");
        std::fs::write(dir.path().join("untitled.md"), "x").expect("write");
        std::fs::write(dir.path().join("Untitled-2.md"), "x").expect("write");
        let ready = prepare_in(
            &args(&[
                "permark",
                NEW_MARKDOWN_FLAG,
                &dir.path().display().to_string(),
            ]),
            None,
        )
        .expect("a launch");
        let file = ready.file.expect("the file");
        assert!(file.to_lowercase().ends_with("untitled-3.md"));
        assert_eq!(
            std::fs::read_to_string(&file).expect("read"),
            "# untitled 3\n"
        );
    }

    #[test]
    fn a_missing_folder_opens_nothing() {
        let dir = tempfile::tempdir().expect("temp");
        let missing = dir.path().join("gone");
        assert!(prepare_in(
            &args(&["permark", NEW_MARKDOWN_FLAG, &missing.display().to_string()]),
            None
        )
        .is_none());
    }
}
