//! Plain spelling of Windows paths: `\\?\` | `\\.\` prefixes dropped where the plain path names the same place, so
//! git, the UI and MCP clients see `C:\x` | `\\server\share\x`. `std::fs::canonicalize` keeps `\\?\UNC\` on every
//! share (WSL folders included) and `dunce` strips only the drive form. Unix: paths unchanged.

use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};

use super::windows_name_ok;

/// Win32 plain paths: fewer than 260 UTF-16 units (`MAX_PATH` incl. NUL); longer ones keep the prefix.
const MAX_PLAIN_UNITS: usize = 260;

/// `path` canonical (links resolved, case as on disk, 8.3 names expanded) in its plain spelling ([`simplified`]).
///
/// # Errors
/// As `std::fs::canonicalize` (missing path, access denied, ...).
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let canon = std::fs::canonicalize(path)?;
    // a borrowed answer may still be shorter (dunce slices the drive form)
    Ok(simplified(&canon).into_owned())
}

/// `path` without its prefix when the plain spelling names the same place: `\\?\C:\x` -> `C:\x` (dunce's rules),
/// `\\?\UNC\server\share\x` | `\\.\UNC\server\share\x` -> `\\server\share\x` (prefix case ignored). Kept as is: a
/// name Win32 reads differently (trailing dot | space, device name, `.` | `..`, `/`, reserved char, empty), 260
/// UTF-16 units or more, no share, other forms (`\\?\Volume{...}\`, `\\?\GLOBALROOT\...`), non-UTF-8 text. Pure.
#[must_use]
pub fn simplified(path: &Path) -> Cow<'_, Path> {
    if !cfg!(windows) {
        return Cow::Borrowed(path);
    }
    if let Some(plain) = path.to_str().and_then(plain_unc) {
        return Cow::Owned(PathBuf::from(plain));
    }
    Cow::Borrowed(dunce::simplified(path))
}

/// [`simplified`] of a path kept as text (saved data, IPC strings).
#[must_use]
pub fn simplified_text(text: &str) -> Cow<'_, str> {
    match simplified(Path::new(text)) {
        Cow::Borrowed(same) if same.as_os_str().len() == text.len() => Cow::Borrowed(text),
        other => Cow::Owned(other.to_string_lossy().into_owned()),
    }
}

/// `prefix` cut from `text`, ASCII case ignored.
pub(crate) fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// `\\?\UNC\server\share[\names][\]` | `\\.\UNC\...` -> `\\server\share...` when every name is one Win32 keeps as
/// written and the result is short enough. Pure text, every OS (tests run on all three).
fn plain_unc(text: &str) -> Option<String> {
    let rest = strip_prefix_ci(text, r"\\?\UNC\").or_else(|| strip_prefix_ci(text, r"\\.\UNC\"))?;
    let body = rest.strip_suffix('\\').unwrap_or(rest);
    let mut names = 0usize;
    for name in body.split('\\') {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || !windows_name_ok(name)
        {
            return None;
        }
        names += 1;
    }
    if names < 2 {
        return None;
    }
    let plain = format!(r"\\{rest}");
    (plain.encode_utf16().count() < MAX_PLAIN_UNITS).then_some(plain)
}

/// `local` (a drive path) through the loopback admin share `\\localhost\X$\...`; `None` where the share is off.
#[cfg(all(test, windows))]
pub(crate) fn via_admin_share(local: &Path) -> Option<PathBuf> {
    let local = dunce::canonicalize(local).ok()?;
    let text = local.to_str()?;
    let (drive, rest) = text.split_once(r":\")?;
    let shared = PathBuf::from(format!(r"\\localhost\{drive}$\{rest}"));
    shared.is_dir().then_some(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_unc_becomes_plain_unc() {
        let cases = [
            (r"\\?\UNC\srv\share\x", r"\\srv\share\x"),
            (r"\\?\UNC\srv\share", r"\\srv\share"),
            (r"\\?\UNC\srv\share\", r"\\srv\share\"),
            (r"\\?\unc\Srv\Share\Dir", r"\\Srv\Share\Dir"),
            (r"\\.\UNC\srv\share\x", r"\\srv\share\x"),
            (
                r"\\?\UNC\wsl.localhost\Ubuntu-24.04\home\edelm\cursos",
                r"\\wsl.localhost\Ubuntu-24.04\home\edelm\cursos",
            ),
            (r"\\?\UNC\wsl$\Ubuntu\tmp", r"\\wsl$\Ubuntu\tmp"),
            (r"\\?\UNC\srv\ação 文档\📁 x", r"\\srv\ação 文档\📁 x"),
        ];
        for (raw, want) in cases {
            assert_eq!(plain_unc(raw).as_deref(), Some(want), "{raw}");
        }
    }

    #[test]
    fn unc_forms_win32_would_read_differently_keep_their_prefix() {
        let long = format!(r"\\?\UNC\srv\share\{}", "d".repeat(260));
        for raw in [
            r"\\?\UNC\srv",
            r"\\?\UNC\srv\",
            r"\\?\UNC\",
            r"\\?\UNC\srv\share\..\x",
            r"\\?\UNC\srv\share\.\x",
            r"\\?\UNC\srv\share\dot.",
            r"\\?\UNC\srv\share\space ",
            r"\\?\UNC\srv\share\CON",
            r"\\?\UNC\srv\share\a\\b",
            r"\\?\UNC\srv\share\a/b",
            r"\\?\UNC\srv\share\a?b",
            r"\\?\UNC\[::1]\share",
            long.as_str(),
            r"\\?\Volume{0b1c2d3e-0000-0000-0000-100000000000}\x",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\x",
            r"\\?\C:\x",
            r"\\srv\share\x",
            r"C:\x",
            "/home/me",
        ] {
            assert_eq!(plain_unc(raw), None, "{raw}");
        }
        let edge = format!(r"\\?\UNC\s\t\{}", "d".repeat(259 - 6));
        assert_eq!(plain_unc(&edge).map(|p| p.len()), Some(259));
        let over = format!(r"\\?\UNC\s\t\{}", "d".repeat(260 - 6));
        assert_eq!(plain_unc(&over), None);
    }

    #[cfg(windows)]
    #[test]
    fn simplified_handles_drive_and_unc_forms() {
        let s = |p: &str| simplified(Path::new(p)).to_string_lossy().into_owned();
        assert_eq!(s(r"\\?\C:\x"), r"C:\x");
        assert_eq!(s(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(s(r"C:\x"), r"C:\x");
        assert_eq!(s(r"\\srv\share\x"), r"\\srv\share\x");
        let volume = r"\\?\Volume{0b1c2d3e-0000-0000-0000-100000000000}\x";
        assert_eq!(s(volume), volume);
        assert!(matches!(simplified(Path::new(r"C:\x")), Cow::Borrowed(_)));
        assert_eq!(simplified_text(r"\\?\UNC\srv\share"), r"\\srv\share");
        assert!(matches!(simplified_text(r"C:\x"), Cow::Borrowed(_)));
    }

    #[cfg(not(windows))]
    #[test]
    fn simplified_leaves_unix_paths_alone() {
        let odd = r"\\?\UNC\srv\share";
        assert_eq!(simplified(Path::new(odd)), Path::new(odd));
        assert_eq!(simplified_text("/home/me"), "/home/me");
    }

    #[cfg(windows)]
    #[test]
    fn a_share_canonicalizes_to_its_plain_spelling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub ü")).unwrap();
        let Some(shared) = via_admin_share(dir.path()) else {
            eprintln!("skipped: no loopback admin share here");
            return;
        };
        // std keeps the verbatim form: the bug this module fixes
        let raw = std::fs::canonicalize(&shared).unwrap();
        assert!(raw.to_string_lossy().starts_with(r"\\?\UNC\"));
        let canon = canonicalize(&shared.join("sub ü").join("..")).unwrap();
        assert!(
            canon.to_string_lossy().starts_with(r"\\localhost\"),
            "{}",
            canon.display()
        );
        assert_eq!(canonicalize(&raw).unwrap(), canon);
        let root = super::super::Root::open(&raw).unwrap();
        assert_eq!(root.path(), canon);
        assert_eq!(root.key(), super::super::Root::open(&shared).unwrap().key());
        // what leaves the backend: absolute paths of files (IPC `file_abs_path`, MCP), a single file opened there
        std::fs::write(shared.join("sub ü").join("a.md"), "x").unwrap();
        let vault = super::super::Vault::new(root);
        let doc = super::super::VaultPath::parse("sub ü/a.md").unwrap();
        let abs = vault.resolve(&doc).unwrap();
        assert!(abs.starts_with(&canon) && !abs.to_string_lossy().contains(r"\\?\"));
        let verbatim_abs = raw.join("sub ü").join("a.md");
        assert_eq!(vault.relativize(&verbatim_abs), Some(doc));
        let single = super::super::FileScope::open(&verbatim_abs).unwrap();
        assert_eq!(single.file(), abs);
        assert_eq!(single.folder().path(), canon.join("sub ü"));
    }

    #[test]
    fn canonicalize_drops_the_prefix_of_local_folders() {
        let dir = tempfile::tempdir().unwrap();
        let canon = canonicalize(dir.path()).unwrap();
        assert!(!canon.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(canon, dunce::canonicalize(dir.path()).unwrap());
    }
}
