//! Reads through the project view of core ([`Guard`]) as an agent hears them: what it may not see answers
//! `not found`, a member's denial names the member and the call's group.

use std::path::PathBuf;

use permark_core::projects::agent_view::{Guard, Refusal};

use crate::perms::Group;
use crate::tools::{read_error, MAX_READ_BYTES};
use crate::ToolError;

/// [`Guard::file`] as a tool answer.
///
/// # Errors
/// `NotFound` with the path as the agent gave it; `DeniedBelow` (a member denies the call's group there).
pub(crate) fn file(guard: &Guard, raw: &str) -> Result<(String, PathBuf), ToolError> {
    guard
        .file(raw)
        .map_err(|refusal| refused(guard, raw, refusal))
}

/// [`file`] and its text: the file is checked again where it really is once open, so a link swapped in after the
/// check of the path and before the open never leads out of the folder or to a hidden part of it.
///
/// # Errors
/// See [`file`]; `Invalid` (too large, not text), `Failed` (unreadable now).
pub(crate) fn text(guard: &Guard, raw: &str) -> Result<(String, String), ToolError> {
    let (path, abs) = file(guard, raw)?;
    let (decoded, opened) =
        permark_core::fsx::read_text_opened(&abs, MAX_READ_BYTES).map_err(|e| read_error(&e))?;
    if !guard.opened_shared(opened.as_deref()) {
        return Err(missing(raw));
    }
    Ok((path, decoded.text))
}

fn missing(raw: &str) -> ToolError {
    ToolError::NotFound(raw.trim().to_owned())
}

fn refused(guard: &Guard, raw: &str, refusal: Refusal) -> ToolError {
    match refusal {
        Refusal::Missing => missing(raw),
        // only a guard narrowed for a group denies below a member; a denial without one stays unnamed
        Refusal::DeniedBelow { member } => guard.deny_group().and_then(Group::parse).map_or_else(
            || missing(raw),
            |group| ToolError::DeniedBelow { group, member },
        ),
    }
}

#[cfg(test)]
mod tests {
    use permark_core::paths::Root;

    use super::*;

    #[test]
    fn a_shared_file_reads_and_a_hidden_one_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in [
            ("docs/a.md", "# A\n"),
            ("private/s.md", "# S\n"),
            (".aiignore", "private/\n"),
        ] {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        let guard = Guard::new(&Root::open(dir.path()).unwrap(), &[]);
        assert_eq!(
            text(&guard, "docs/a.md").unwrap(),
            ("docs/a.md".to_owned(), "# A\n".to_owned())
        );
        assert_eq!(
            text(&guard, " private/s.md "),
            Err(ToolError::NotFound("private/s.md".to_owned()))
        );
    }
}
