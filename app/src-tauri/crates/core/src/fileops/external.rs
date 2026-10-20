//! Opening a project file outside PerMark: with the system's default program, in the browser, or shown in the file
//! manager. The safe-by-default rule lives here: the path goes through the vault (nothing outside the project), the
//! target must be a plain document-like file (never an executable | script), and the shell hands the program the
//! absolute path only, with no shell involved.

use std::path::PathBuf;

use serde::Deserialize;
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::paths::{Vault, VaultPath};
use crate::tree::{is_html, may_open_with_default_app};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OpenMode {
    /// The program the system has for this kind of file (documents, pictures, sound, video, office files).
    DefaultApp,
    /// `.html | .htm` in the browser.
    Browser,
    /// The file manager, with the item selected.
    Reveal,
}

/// The absolute place to hand to the opener, or why not.
///
/// # Errors
/// `PathEscape` | `NotFound` | `InvalidPath` from the vault; `Unsupported` for a kind of file that is never opened from
/// a click (executables, scripts, links that lead to one, folders for the default program).
pub fn external_target(vault: &Vault, path: &str, mode: OpenMode) -> Result<PathBuf, IpcError> {
    let parsed = VaultPath::parse(path).map_err(IpcError::from)?;
    // `resolve` follows links: a link inside the project that leads outside is refused here
    let abs = vault.resolve(&parsed).map_err(IpcError::from)?;
    let meta = std::fs::metadata(&abs).map_err(|e| IpcError::io(path, &e))?;
    let refuse = || IpcError::new(ErrorKind::Unsupported).subject(path);
    if mode == OpenMode::Reveal {
        return Ok(abs);
    }
    if !meta.is_file() {
        return Err(refuse());
    }
    // both the name given and the name of what it leads to must qualify (`notes.md` linking to `tool.exe` does not)
    let shown = parsed.name().unwrap_or("");
    let real = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let allowed = |name: &str| match mode {
        OpenMode::DefaultApp => may_open_with_default_app(name),
        OpenMode::Browser => is_html(name),
        OpenMode::Reveal => true,
    };
    if allowed(shown) && allowed(real) {
        Ok(abs)
    } else {
        Err(refuse())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::paths::Root;

    fn vault(files: &[&str]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let p = dir.path().join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "x").unwrap();
        }
        let vault = Vault::new(Root::open(dir.path()).unwrap());
        (dir, vault)
    }

    fn kind(result: Result<PathBuf, IpcError>) -> Option<ErrorKind> {
        result.err().map(|e| e.kind)
    }

    #[test]
    fn documents_open_with_the_default_program_and_scripts_never_do() {
        let (_d, v) = vault(&[
            "a.md",
            "doc/Report Q1.docx",
            "p.png",
            "run.sh",
            "setup.exe",
            "x.bat",
            "evil.js",
            "page.html",
            "pic.svg",
            "noext",
            "-rf.md",
        ]);
        for ok in ["a.md", "doc/Report Q1.docx", "p.png", "-rf.md"] {
            let abs = external_target(&v, ok, OpenMode::DefaultApp).unwrap();
            assert!(
                abs.is_absolute(),
                "{ok}: handed over absolute, never starting with a dash"
            );
        }
        for no in [
            "run.sh",
            "setup.exe",
            "x.bat",
            "evil.js",
            "page.html",
            "pic.svg",
            "noext",
        ] {
            assert_eq!(
                kind(external_target(&v, no, OpenMode::DefaultApp)),
                Some(ErrorKind::Unsupported),
                "{no}"
            );
        }
    }

    #[test]
    fn the_browser_gets_html_only() {
        let (_d, v) = vault(&["page.html", "old.HTM", "a.md", "run.exe"]);
        assert!(external_target(&v, "page.html", OpenMode::Browser).is_ok());
        assert!(external_target(&v, "old.HTM", OpenMode::Browser).is_ok());
        for no in ["a.md", "run.exe"] {
            assert_eq!(
                kind(external_target(&v, no, OpenMode::Browser)),
                Some(ErrorKind::Unsupported)
            );
        }
    }

    #[test]
    fn a_folder_is_revealed_never_opened_and_escapes_are_refused() {
        let (_d, v) = vault(&["docs/a.md"]);
        assert!(external_target(&v, "docs", OpenMode::Reveal).is_ok());
        assert!(external_target(&v, "docs/a.md", OpenMode::Reveal).is_ok());
        assert_eq!(
            kind(external_target(&v, "docs", OpenMode::DefaultApp)),
            Some(ErrorKind::Unsupported)
        );
        assert_eq!(
            kind(external_target(&v, "../x.md", OpenMode::DefaultApp)),
            Some(ErrorKind::PathEscape)
        );
        assert_eq!(
            kind(external_target(&v, "missing.md", OpenMode::DefaultApp)),
            Some(ErrorKind::NotFound)
        );
        assert_eq!(
            kind(external_target(
                &v,
                "C:/Windows/notepad.exe",
                OpenMode::Reveal
            )),
            Some(ErrorKind::InvalidPath)
        );
        assert_eq!(
            kind(external_target(&v, "/etc/passwd", OpenMode::Reveal)),
            Some(ErrorKind::InvalidPath)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_document_name_that_links_to_a_program_or_outside_is_refused() {
        let (dir, v) = vault(&["tool.exe"]);
        std::os::unix::fs::symlink(dir.path().join("tool.exe"), dir.path().join("notes.md"))
            .unwrap();
        assert_eq!(
            kind(external_target(&v, "notes.md", OpenMode::DefaultApp)),
            Some(ErrorKind::Unsupported)
        );
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("s.md"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path().join("s.md"), dir.path().join("leak.md"))
            .unwrap();
        assert_eq!(
            kind(external_target(&v, "leak.md", OpenMode::DefaultApp)),
            Some(ErrorKind::PathEscape)
        );
    }
}
