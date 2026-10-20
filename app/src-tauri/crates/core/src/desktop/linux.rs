//! Linux: the desktop entry PerMark was installed with, and the person's `mimeapps.list`.

use std::path::{Path, PathBuf};

use super::{default_of, with_default, FileGroup, Handlers};
use crate::{fsx, IpcError};

/// The desktop entry the Linux packages install (`<productName>.desktop`).
pub const DESKTOP_ID: &str = "PerMark.desktop";

/// How this copy runs on Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Installed with a desktop entry (a .deb | .rpm package).
    Desktop,
    /// A snap: confined, the person's settings are out of reach.
    Snap,
    /// Not installed (an AppImage | a build started from a folder).
    Loose,
}

/// An absolute path as Linux writes it (also on the computers the tests run on).
fn absolute(path: &str) -> bool {
    path.starts_with('/')
}

/// What the environment says about this copy (`var`: the environment, `exists`: the file system; injected for tests).
#[must_use]
pub fn install(var: impl Fn(&str) -> Option<String>, exists: impl Fn(&Path) -> bool) -> Install {
    if var("SNAP").is_some_and(|v| !v.is_empty()) {
        return Install::Snap;
    }
    if var("APPIMAGE").is_some_and(|v| !v.is_empty()) {
        return Install::Loose;
    }
    let data_home = var("XDG_DATA_HOME")
        .filter(|v| absolute(v))
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| Path::new(&home).join(".local/share")));
    let data_dirs = var("XDG_DATA_DIRS")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    let found = data_home
        .into_iter()
        .chain(
            data_dirs
                .split(':')
                .filter(|d| absolute(d))
                .map(PathBuf::from),
        )
        .any(|dir| exists(&dir.join("applications").join(DESKTOP_ID)));
    if found {
        Install::Desktop
    } else {
        Install::Loose
    }
}

/// The person's `mimeapps.list` (`$XDG_CONFIG_HOME`, else `~/.config`).
#[must_use]
pub fn mimeapps_path(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    var("XDG_CONFIG_HOME")
        .filter(|v| absolute(v))
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| Path::new(&home).join(".config")))
        .map(|dir| dir.join("mimeapps.list"))
}

/// Default handlers recorded in one `mimeapps.list`.
pub struct MimeApps {
    path: PathBuf,
    me: String,
}

impl MimeApps {
    #[must_use]
    pub fn new(path: PathBuf, me: impl Into<String>) -> Self {
        Self {
            path,
            me: me.into(),
        }
    }

    fn text(&self) -> Result<String, IpcError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
                IpcError::new(crate::ErrorKind::Encoding).subject(self.path.display().to_string())
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(IpcError::io(self.path.display().to_string(), &e)),
        }
    }
}

impl Handlers for MimeApps {
    fn keys(&self, group: FileGroup) -> Vec<String> {
        group.mime_types().iter().map(|m| (*m).to_owned()).collect()
    }

    /// The value as written, or PerMark's id when PerMark comes first in it.
    fn get(&self, key: &str) -> Option<String> {
        let value = default_of(&self.text().ok()?, key)?;
        let first = value.split(';').map(str::trim).find(|e| !e.is_empty())?;
        if first.eq_ignore_ascii_case(&self.me) {
            Some(self.me.clone())
        } else {
            Some(value)
        }
    }

    fn set(&self, key: &str, handler: Option<&str>) -> Result<(), IpcError> {
        let text = self.text()?;
        let value = handler.map(|h| {
            if h.eq_ignore_ascii_case(&self.me) {
                format!("{h};")
            } else {
                h.to_owned()
            }
        });
        let changed = with_default(&text, key, value.as_deref());
        if changed == text {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| IpcError::io(dir.display().to_string(), &e))?;
        }
        fsx::write_atomic(&self.path, changed.as_bytes())
            .map_err(|e| IpcError::io(self.path.display().to_string(), &e))
    }

    fn me(&self) -> &str {
        &self.me
    }
}
