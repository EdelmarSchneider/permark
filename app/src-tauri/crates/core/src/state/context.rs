//! `agent-context.json` in a project's state folder: the live context the agent connection publishes for scripts
//! without MCP (the path stays: scripts read it there). It carries a token only this install can make for this folder
//! and this content, so a copy that arrived with the folder (a zip, a clone, a sync partner), or one whose content
//! changed since, is told apart from the one this install wrote, and is removed when the project opens, whether the
//! agent connection is on or not.

use std::io;

use serde_json::{Map, Value};

use super::{Machine, Seal, StateLocation, StateStore};

/// Name of the file in the project's state folder (`.permark/local/`).
pub const FILE: &str = "agent-context.json";
/// Seal kind of the token: bound to the folder's id under this install's key, never to the install id alone (that one
/// names a folder inside `.permark/local/` and travels with it).
const KIND: &str = "agent-context";
/// Key of the token in the context object.
const TOKEN: &str = "token";
/// Most bytes the file holds: the open check reads no more (a published context is a few KiB: its selection and
/// headings are cut short), and the writer keeps no larger file ([`encode`]).
pub const MAX_BYTES: u64 = 64 * 1024;

/// The seal of `context` (the object without its token) for the folder: its kind, the folder id, then the object as
/// compact JSON, keys in the object's own order (the writer's order, which a read of the file keeps).
fn seal(machine: &Machine, context: &Map<String, Value>) -> Option<Seal> {
    let bytes = serde_json::to_vec(context).ok()?;
    let mut seal = machine.seal(KIND);
    seal.field(&bytes);
    Some(seal)
}

/// The token this install writes into `context` (the object without its token) for the store's folder; `None`: no
/// usable install key, or the app-wide store.
#[must_use]
pub fn token(store: &StateStore, context: &Map<String, Value>) -> Option<String> {
    seal(store.machine()?, context).map(|seal| seal.sign())
}

/// The bytes of the file for `context`: compact JSON with this install's token last (none without a usable install
/// key: such a file is then left at open, as its token cannot be checked either). `None`: larger than [`MAX_BYTES`],
/// which the open check would take for a copy that came with the folder; write no file then (none rather than an
/// older one).
#[must_use]
pub fn encode(store: &StateStore, mut context: Map<String, Value>) -> Option<Vec<u8>> {
    context.shift_remove(TOKEN);
    if let Some(token) = token(store, &context) {
        context.insert(TOKEN.to_owned(), Value::from(token));
    }
    serde_json::to_vec(&context)
        .ok()
        .filter(|bytes| bytes.len() as u64 <= MAX_BYTES)
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
    // made at every project open anyway (the undo journal asks for it); none usable: nothing can be checked
    let Some(machine) = store.machine() else {
        return;
    };
    let ours = bytes.is_some_and(|bytes| {
        serde_json::from_slice::<Map<String, Value>>(&bytes).is_ok_and(|mut context| {
            let sealed = match context.shift_remove(TOKEN) {
                Some(Value::String(token)) => {
                    seal(machine, &context).is_some_and(|seal| seal.verify(&token))
                }
                _ => false,
            };
            // and exactly the bytes this install writes: the token seals the object serde reads, while another
            // parser may read other content from other bytes (a key given twice: serde keeps the last value). Checked
            // after the token, so a comparison that stops at the first differing byte never tells anything of it.
            sealed && encode(store, context).is_some_and(|written| written == bytes)
        })
    });
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

    /// A context object as the agent connection publishes it, without a token.
    fn map() -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("v".to_owned(), Value::from(1));
        map.insert("doc".to_owned(), Value::from("docs/plan.md"));
        map
    }

    /// The file `store`'s install writes for [`map`].
    fn ours(store: &StateStore) -> Vec<u8> {
        encode(store, map()).unwrap()
    }

    /// The file of [`map`] with `token` when given, as anyone may write it.
    fn context(token: Option<&str>) -> Vec<u8> {
        let mut map = map();
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
    fn the_token_is_bound_to_this_install_this_folder_and_this_content() {
        let f = fx();
        let store = f.store();
        let token = token(&store, &map()).unwrap();
        assert!(token.starts_with("s1:") && token.len() == 35, "{token}");
        assert_eq!(
            super::token(&f.store(), &map()).as_deref(),
            Some(token.as_str())
        );
        let other = tempfile::tempdir().unwrap();
        let other = StateStore::for_project(&f.dirs, &Root::open(other.path()).unwrap());
        assert_ne!(
            super::token(&other, &map()).unwrap(),
            token,
            "another folder"
        );
        let base = tempfile::tempdir().unwrap();
        let elsewhere = StateStore::for_project(&AppDirs::under(base.path()), &f.root());
        assert_ne!(
            super::token(&elsewhere, &map()).unwrap(),
            token,
            "another install"
        );
        assert!(super::token(&StateStore::app(&f.dirs), &map()).is_none());
        let mut changed = map();
        changed.insert("doc".to_owned(), Value::from("docs/other.md"));
        assert_ne!(
            super::token(&store, &changed).unwrap(),
            token,
            "another content"
        );
        // the file: the context as published, its token last; a token already in the object is never sealed
        let bytes = ours(&store);
        let written: Map<String, Value> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(written.keys().collect::<Vec<_>>(), ["v", "doc", "token"]);
        assert_eq!(written["token"], Value::from(token));
        let mut stale = map();
        stale.insert("token".to_owned(), Value::from("s1:0"));
        assert_eq!(encode(&store, stale), Some(bytes));
    }

    /// The writer and the open check agree on the most bytes: a file of exactly [`MAX_BYTES`] is written and stays,
    /// one byte more is never written (the check would take it for a copy that came with the folder).
    #[test]
    fn the_writer_keeps_no_file_the_open_check_would_remove() {
        let f = fx();
        let store = f.store();
        let sized = |pad: usize| {
            let mut map = map();
            map.insert("selection".to_owned(), Value::from("x".repeat(pad)));
            map
        };
        let short = encode(&store, sized(0)).unwrap().len();
        let pad = usize::try_from(MAX_BYTES).unwrap() - short;
        let largest = encode(&store, sized(pad)).unwrap();
        assert_eq!(largest.len() as u64, MAX_BYTES);
        put(&file(&store), &largest);
        remove_foreign(&store);
        assert_eq!(fs::read(file(&store)).unwrap(), largest, "kept");
        assert_eq!(encode(&store, sized(pad + 1)), None);
    }

    #[test]
    fn only_the_copy_this_install_wrote_for_this_folder_stays_at_open() {
        let f = fx();
        let store = f.store();
        assert_eq!(store.location(), StateLocation::Project);
        let path = file(&store);
        let ours = ours(&store);
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
            ("another folder", self::ours(&other_folder)),
            ("another install", self::ours(&other_install)),
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

    /// The token seals the content: a copy that kept it but changed what the context says is not this install's.
    #[test]
    fn a_copy_whose_content_changed_is_removed_at_open() {
        let f = fx();
        let store = f.store();
        let path = file(&store);
        let ours = ours(&store);
        let mut map: Map<String, Value> = serde_json::from_slice(&ours).unwrap();
        put(&path, &ours);
        remove_foreign(&store);
        assert_eq!(fs::read(&path).unwrap(), ours, "unchanged: kept");
        map.insert("doc".to_owned(), Value::from("docs/other.md"));
        put(&path, &serde_json::to_vec(&map).unwrap());
        remove_foreign(&store);
        assert!(!path.exists(), "same token, other content: removed");
    }

    /// Only the exact bytes this install writes stay. A key given twice: serde keeps the last value, which the token
    /// sealed, while a parser that keeps the first reads other content. The same content with other whitespace goes
    /// too, on purpose: nothing but this install's own output is kept.
    #[test]
    fn only_the_exact_bytes_this_install_writes_stay_at_open() {
        let f = fx();
        let store = f.store();
        let path = file(&store);
        let token = token(&store, &map()).unwrap();
        let doubled = format!(r#"{{"v":1,"doc":"x","doc":"docs/plan.md","token":"{token}"}}"#);
        let read: Map<String, Value> = serde_json::from_slice(doubled.as_bytes()).unwrap();
        assert_eq!(read["doc"], Value::from("docs/plan.md"), "the sealed value");
        put(&path, doubled.as_bytes());
        remove_foreign(&store);
        assert!(!path.exists(), "a key given twice: removed");
        let spaced = format!(r#"{{ "v": 1, "doc": "docs/plan.md", "token": "{token}" }}"#);
        put(&path, spaced.as_bytes());
        remove_foreign(&store);
        assert!(!path.exists(), "other whitespace: removed");
        let ours = ours(&store);
        put(&path, &ours);
        remove_foreign(&store);
        assert_eq!(fs::read(&path).unwrap(), ours, "this install's bytes: kept");
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
        let ours = ours(&store);
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
        let ours = ours(&f.store());
        put(&path, &ours);
        drop(ProjectCore::open(f.root(), &env));
        assert_eq!(fs::read(&path).unwrap(), ours, "this install's copy stays");
    }
}
