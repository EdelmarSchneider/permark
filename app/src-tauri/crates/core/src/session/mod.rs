//! Session: app session (windows + geometry), recent folders, per-project session values owned by features.

mod place;

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use place::{place, Geometry, Limits, Placement, Rect, Screen};

use crate::error::{ErrorKind, IpcError};
use crate::paths::Root;
use crate::state::{Loaded, StateData, StateError, StateFile, StateStore};
use crate::workspace::WorkspaceRefDto;

/// One window as it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub workspace: Option<WorkspaceRefDto>,
    pub geometry: Option<Geometry>,
}

/// `<profile>/session.json`: the windows to bring back, oldest first.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AppSession {
    pub windows: Vec<SavedWindow>,
}

impl StateData for AppSession {
    const VERSION: u32 = 1;
}

/// The app session file.
#[must_use]
pub fn app_session_file(store: &StateStore) -> StateFile<AppSession> {
    store.file("session.json")
}

/// Most recent folders kept.
pub const MAX_RECENT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecentEntry {
    path: String,
    name: String,
    opened_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct RecentList {
    entries: Vec<RecentEntry>,
}

impl StateData for RecentList {
    const VERSION: u32 = 1;
}

/// A recent folder as the welcome screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentFolder {
    pub path: String,
    pub name: String,
    #[ts(type = "number")]
    pub opened_ms: u64,
    /// Folder gone (never auto-removed: a drive may come back).
    pub missing: bool,
}

/// `<profile>/recent.json`.
#[derive(Debug)]
pub struct RecentStore {
    file: StateFile<RecentList>,
}

impl RecentStore {
    #[must_use]
    pub fn new(app: &StateStore) -> Self {
        Self {
            file: app.file("recent.json"),
        }
    }

    /// Newest first, missing ones marked. Corrupt file -> empty (backup kept).
    #[must_use]
    pub fn list(&self) -> Vec<RecentFolder> {
        self.load()
            .entries
            .into_iter()
            .map(|e| RecentFolder {
                missing: !Path::new(&e.path).is_dir(),
                path: e.path,
                name: e.name,
                opened_ms: e.opened_ms,
            })
            .collect()
    }

    fn load(&self) -> RecentList {
        match self.file.load() {
            Loaded::Found(list) => list,
            Loaded::Missing | Loaded::Recovered { .. } => RecentList::default(),
        }
    }

    /// Moves | adds a project to the top (dedup by `RootKey`), capped at [`MAX_RECENT`].
    ///
    /// # Errors
    /// Write failure.
    pub fn touch(&self, root: &Root, now_ms: u64) -> Result<(), StateError> {
        let mut list = self.load();
        list.entries.retain(|e| {
            Root::open(Path::new(&e.path)).map_or_else(
                |_| {
                    crate::paths::fold(&e.path)
                        != crate::paths::fold(&root.path().display().to_string())
                },
                |r| r != *root,
            )
        });
        list.entries.insert(
            0,
            RecentEntry {
                path: root.path().display().to_string(),
                name: root.name(),
                opened_ms: now_ms,
            },
        );
        list.entries.truncate(MAX_RECENT);
        self.file.save(&list)
    }

    /// Removes an entry by its path as listed (missing folders too).
    ///
    /// # Errors
    /// Write failure.
    pub fn remove(&self, path: &str) -> Result<(), StateError> {
        let mut list = self.load();
        let before = list.entries.len();
        list.entries.retain(|e| e.path != path);
        if list.entries.len() == before {
            return Ok(());
        }
        self.file.save(&list)
    }
}

/// Largest value one feature may keep under its key.
pub const MAX_VALUE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct ProjectSessionData {
    values: BTreeMap<String, serde_json::Value>,
}

impl StateData for ProjectSessionData {
    const VERSION: u32 = 1;
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session value {key} is too large ({bytes} bytes)")]
    TooLarge { key: String, bytes: usize },
    #[error("invalid session key {0:?}")]
    InvalidKey(String),
    #[error(transparent)]
    State(#[from] StateError),
}

impl From<SessionError> for IpcError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::TooLarge { key, .. } => IpcError::new(ErrorKind::TooLarge).subject(key),
            SessionError::InvalidKey(key) => IpcError::new(ErrorKind::InvalidData).subject(key),
            SessionError::State(e) => e.into(),
        }
    }
}

/// Per-project session (`session.json` in the project state dir): `key -> JSON value`, each key owned by one feature.
#[derive(Debug)]
pub struct ProjectSession {
    file: StateFile<ProjectSessionData>,
}

/// Keys: `[a-z0-9.-]`, 1..=64 chars (`explorer.expanded`, `doc.scroll`).
fn valid_key(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

impl ProjectSession {
    #[must_use]
    pub fn new(project: &StateStore) -> Self {
        Self {
            file: project.file("session.json"),
        }
    }

    /// # Errors
    /// Invalid key.
    pub fn get(&self, key: &str) -> Result<Option<serde_json::Value>, SessionError> {
        if !valid_key(key) {
            return Err(SessionError::InvalidKey(key.to_owned()));
        }
        Ok(self.file.load().into_value().values.remove(key))
    }

    /// Sets (`Some`) | clears (`None`) one key; other keys untouched.
    ///
    /// # Errors
    /// Invalid key, value too large, write failure.
    pub fn set(&self, key: &str, value: Option<serde_json::Value>) -> Result<(), SessionError> {
        if !valid_key(key) {
            return Err(SessionError::InvalidKey(key.to_owned()));
        }
        let mut data = self.file.load().into_value();
        match value {
            Some(value) => {
                let bytes = serde_json::to_vec(&value).map_or(usize::MAX, |b| b.len());
                if bytes > MAX_VALUE_BYTES {
                    return Err(SessionError::TooLarge {
                        key: key.to_owned(),
                        bytes,
                    });
                }
                data.values.insert(key.to_owned(), value);
            }
            None => {
                if data.values.remove(key).is_none() {
                    return Ok(());
                }
            }
        }
        Ok(self.file.save(&data)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppDirs;

    fn app() -> (tempfile::TempDir, StateStore) {
        let base = tempfile::tempdir().unwrap();
        let store = StateStore::app(&AppDirs::under(base.path()));
        (base, store)
    }

    #[test]
    fn recent_newest_first_dedup_by_root_and_capped() {
        let (_b, store) = app();
        let recent = RecentStore::new(&store);
        let dirs: Vec<tempfile::TempDir> = (0..25).map(|_| tempfile::tempdir().unwrap()).collect();
        for (i, d) in dirs.iter().enumerate() {
            recent
                .touch(&Root::open(d.path()).unwrap(), i as u64)
                .unwrap();
        }
        // same folder spelled differently -> one entry, moved to top
        let again = Root::open(&dirs[10].path().join(".")).unwrap();
        recent.touch(&again, 99).unwrap();
        let list = recent.list();
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].opened_ms, 99);
        assert_eq!(list.iter().filter(|r| r.path == list[0].path).count(), 1);
        assert_eq!(list[1].opened_ms, 24);
    }

    #[test]
    fn missing_folders_are_marked_kept_and_removable() {
        let (_b, store) = app();
        let recent = RecentStore::new(&store);
        let gone = tempfile::tempdir().unwrap();
        let root = Root::open(gone.path()).unwrap();
        recent.touch(&root, 1).unwrap();
        let path = root.path().display().to_string();
        drop(gone);
        let list = recent.list();
        assert!(list[0].missing);
        // touching another folder keeps the missing one
        let other = tempfile::tempdir().unwrap();
        recent.touch(&Root::open(other.path()).unwrap(), 2).unwrap();
        assert_eq!(recent.list().len(), 2);
        recent.remove(&path).unwrap();
        assert_eq!(recent.list().len(), 1);
        recent.remove("never there").unwrap();
    }

    #[test]
    fn corrupt_recent_file_reads_empty() {
        let (_b, store) = app();
        std::fs::create_dir_all(store.dir()).unwrap();
        std::fs::write(store.dir().join("recent.json"), "[[[").unwrap();
        assert_eq!(RecentStore::new(&store).list().len(), 0);
    }

    #[test]
    fn project_session_keys_are_independent_and_bounded() {
        let (_b, store) = app();
        let session = ProjectSession::new(&store);
        session
            .set("doc.scroll", Some(serde_json::json!({ "anchor": "h-1" })))
            .unwrap();
        session
            .set("explorer.expanded", Some(serde_json::json!(["a", "b"])))
            .unwrap();
        assert_eq!(
            session.get("doc.scroll").unwrap(),
            Some(serde_json::json!({ "anchor": "h-1" }))
        );
        session.set("doc.scroll", None).unwrap();
        assert_eq!(session.get("doc.scroll").unwrap(), None);
        assert!(session.get("explorer.expanded").unwrap().is_some());
        for bad in ["", "Doc", "a b", "../x", &"k".repeat(65)] {
            assert!(
                matches!(session.get(bad), Err(SessionError::InvalidKey(_))),
                "{bad}"
            );
        }
        let big = serde_json::Value::String("x".repeat(MAX_VALUE_BYTES));
        assert!(matches!(
            session.set("big", Some(big)),
            Err(SessionError::TooLarge { .. })
        ));
    }

    #[test]
    fn app_session_round_trips() {
        let (_b, store) = app();
        let file = app_session_file(&store);
        let session = AppSession {
            windows: vec![SavedWindow {
                workspace: Some(WorkspaceRefDto::Project {
                    path: "C:/x y/项目".to_owned(),
                }),
                geometry: Some(Geometry {
                    x: -10,
                    y: 5,
                    width: 1200,
                    height: 800,
                    scale: 1.25,
                    maximized: true,
                }),
            }],
        };
        file.save(&session).unwrap();
        assert_eq!(file.load(), Loaded::Found(session));
    }
}
