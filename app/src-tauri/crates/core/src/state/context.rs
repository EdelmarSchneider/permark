//! `agent-context.json` in a project's state folder: the live context the agent connection publishes for scripts
//! without MCP (the path stays: scripts read it there). It carries a token only this install can make for this folder,
//! so a copy that arrived with the folder (a zip, a clone, a sync partner) is told apart from the one this install
//! wrote, and is removed when the project opens, whether the agent connection is on or not.

use std::io;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{StateLocation, StateStore};

/// Name of the file in the project's state folder (`.permark/local/`).
pub const FILE: &str = "agent-context.json";
/// Seal kind of the token: bound to the folder's id under this install's key, never to the install id alone (that one
/// names a folder inside `.permark/local/` and travels with it).
const KIND: &str = "agent-context";
/// Most bytes read: a published context is a few KiB (its selection and headings are cut short).
const MAX_BYTES: u64 = 64 * 1024;

/// The part of the file the check reads.
#[derive(Deserialize)]
struct Stamped {
    token: Option<String>,
}

/// The token this install writes into the file for the store's folder; `None`: no usable install key, or the app-wide
/// store.
#[must_use]
pub fn token(store: &StateStore) -> Option<String> {
    store.machine().map(|machine| machine.seal(KIND).sign())
}

/// Adds this install's token to the context object about to be written (nothing without a usable install key: such a
/// file is then left at open, as its token cannot be checked either).
pub fn stamp(store: &StateStore, context: &mut Map<String, Value>) {
    if let Some(token) = token(store) {
        context.insert("token".to_owned(), Value::from(token));
    }
}

/// Removes a context file this install did not write for this folder; run when the project opens, before any plugin
/// starts. Only in the project's own state folder (the agent connection writes none in the profile). Left as it is
/// when its token cannot be checked: no usable install key, or a read that failed for another reason than what stands
/// there (the file held by another program). Best effort: a folder that cannot be written still opens.
pub fn remove_foreign(store: &StateStore) {
    if store.location() != StateLocation::Project {
        return;
    }
    let file = store.raw_file(FILE).capped(MAX_BYTES);
    let bytes = match file.read() {
        Ok(Some(bytes)) => Some(bytes),
        // a link, FIFO, folder or oversized file: never a file this install wrote
        Err(error) if error.kind() == io::ErrorKind::InvalidData => None,
        // nothing there, or a read that may work a moment later
        Ok(None) | Err(_) => return,
    };
    // asked only once a file is there: opening a folder without one never makes the install key
    let Some(machine) = store.machine() else {
        return;
    };
    let ours = bytes
        .and_then(|bytes| serde_json::from_slice::<Stamped>(&bytes).ok())
        .and_then(|stamped| stamped.token)
        .is_some_and(|token| machine.seal(KIND).verify(&token));
    if !ours {
        // removes a link itself; a folder is refused and stays with what it holds
        let _ = file.remove();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::*;
    use crate::paths::Root;
    use crate::state::{AppDirs, StateLocation};

    struct Fx {
        _base: tempfile::TempDir,
        dirs: AppDirs,
        project: tempfile::TempDir,
    }

    fn fx() -> Fx {
        let base = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(base.path());
        let project = tempfile::tempdir().unwrap();
        fs::create_dir(project.path().join(".permark")).unwrap();
        Fx {
            _base: base,
            dirs,
            project,
        }
    }

    impl Fx {
        fn root(&self) -> Root {
            Root::open(self.project.path()).unwrap()
        }
        fn store(&self) -> StateStore {
            StateStore::for_project(&self.dirs, &self.root())
        }
    }

    fn file(store: &StateStore) -> PathBuf {
        store.dir().join(FILE)
    }

    /// A context as the agent connection writes it, with `token` when given.
    fn context(token: Option<&str>) -> Vec<u8> {
        let mut map = Map::new();
        map.insert("v".to_owned(), Value::from(1));
        map.insert("doc".to_owned(), Value::from("docs/plan.md"));
        if let Some(token) = token {
            map.insert("token".to_owned(), Value::from(token));
        }
        serde_json::to_vec(&map).unwrap()
    }

    fn put(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn the_token_is_bound_to_this_install_and_this_folder() {
        let f = fx();
        let store = f.store();
        let token = token(&store).unwrap();
        assert!(token.starts_with("s1:") && token.len() == 35, "{token}");
        assert_eq!(super::token(&f.store()).as_deref(), Some(token.as_str()));
        let other = tempfile::tempdir().unwrap();
        let other = StateStore::for_project(&f.dirs, &Root::open(other.path()).unwrap());
        assert_ne!(super::token(&other).unwrap(), token, "another folder");
        let base = tempfile::tempdir().unwrap();
        let elsewhere = StateStore::for_project(&AppDirs::under(base.path()), &f.root());
        assert_ne!(super::token(&elsewhere).unwrap(), token, "another install");
        assert!(super::token(&StateStore::app(&f.dirs)).is_none());
        let mut map = Map::new();
        stamp(&store, &mut map);
        assert_eq!(map.get("token"), Some(&Value::from(token)));
    }

    #[test]
    fn only_the_copy_this_install_wrote_for_this_folder_stays_at_open() {
        let f = fx();
        let store = f.store();
        assert_eq!(store.location(), StateLocation::Project);
        let path = file(&store);
        let ours = context(token(&store).as_deref());
        put(&path, &ours);
        remove_foreign(&store);
        assert_eq!(fs::read(&path).unwrap(), ours, "kept as it is");

        let other = tempfile::tempdir().unwrap();
        let other_folder = StateStore::for_project(&f.dirs, &Root::open(other.path()).unwrap());
        let base = tempfile::tempdir().unwrap();
        let other_install = StateStore::for_project(&AppDirs::under(base.path()), &f.root());
        let mut padded = ours.clone();
        padded.extend(std::iter::repeat_n(b' ', 70 * 1024));
        let foreign: Vec<(&str, Vec<u8>)> = vec![
            ("no token", context(None)),
            ("another folder", context(token(&other_folder).as_deref())),
            ("another install", context(token(&other_install).as_deref())),
            (
                "a made-up token",
                context(Some("s1:00000000000000000000000000000000")),
            ),
            ("not JSON", b"{ not json".to_vec()),
            ("not an object", b"[1]".to_vec()),
            ("a token that is no text", b"{\"token\": 7}".to_vec()),
            ("larger than any context", padded),
        ];
        for (case, bytes) in foreign {
            put(&path, &bytes);
            remove_foreign(&store);
            assert!(!path.exists(), "{case}: removed");
        }
        // nothing else in the state folder is touched
        put(&store.dir().join("session.json"), b"{}");
        put(&path, &context(None));
        remove_foreign(&store);
        assert_eq!(fs::read(store.dir().join("session.json")).unwrap(), b"{}");
    }

    #[test]
    fn a_file_that_cannot_be_checked_or_lies_outside_the_project_is_left() {
        // no `.permark/`: the state folder is the profile, where the agent connection never writes one
        let f = fx();
        fs::remove_dir(f.project.path().join(".permark")).unwrap();
        let profile = f.store();
        assert_eq!(profile.location(), StateLocation::Profile);
        put(&file(&profile), &context(None));
        remove_foreign(&profile);
        assert!(file(&profile).exists(), "profile copy left");

        // no usable install key: nothing can be checked
        let f = fx();
        let base = tempfile::tempdir().unwrap();
        let mut dirs = AppDirs::under(base.path());
        fs::write(base.path().join("blocked"), "x").unwrap();
        dirs.install = base.path().join("blocked").join("install");
        let blocked = StateStore::for_project(&dirs, &f.root());
        assert!(blocked.machine().is_none());
        put(&file(&blocked), &context(None));
        remove_foreign(&blocked);
        assert!(file(&blocked).exists(), "left without a key");

        // a folder at the name is never removed with what it holds
        let store = f.store();
        fs::remove_file(file(&store)).unwrap();
        fs::create_dir_all(file(&store)).unwrap();
        fs::write(file(&store).join("kept"), b"x").unwrap();
        remove_foreign(&store);
        assert_eq!(fs::read(file(&store).join("kept")).unwrap(), b"x");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_file_is_removed_never_followed() {
        let f = fx();
        let store = f.store();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("context.json");
        let ours = context(token(&store).as_deref());
        fs::write(&target, &ours).unwrap();
        fs::create_dir_all(store.dir()).unwrap();
        std::os::unix::fs::symlink(&target, file(&store)).unwrap();
        remove_foreign(&store);
        assert!(fs::symlink_metadata(file(&store)).is_err(), "link removed");
        assert_eq!(fs::read(&target).unwrap(), ours, "target untouched");
    }

    /// The core removes a copy that came with the folder when the project opens, with no agent plugin at all.
    #[test]
    fn opening_a_project_removes_a_copy_that_came_with_the_folder() {
        use crate::plugins::PluginRegistry;
        use crate::settings::AppSettings;
        use crate::workspace::{CoreEnv, ProjectCore};

        let f = fx();
        let env = CoreEnv::new(
            f.dirs.clone(),
            Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
            Arc::new(crate::events::NoEvents),
            crate::conditions::Distribution::Direct,
            AppSettings::default(),
        );
        let path = file(&f.store());
        put(&path, &context(None));
        drop(ProjectCore::open(f.root(), &env));
        assert!(!path.exists(), "a copy without this install's token goes");
        let ours = context(token(&f.store()).as_deref());
        put(&path, &ours);
        drop(ProjectCore::open(f.root(), &env));
        assert_eq!(fs::read(&path).unwrap(), ours, "this install's copy stays");
    }
}
