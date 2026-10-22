//! Where the person's state of each path lives (`projects.md` §9): the opened folder's store, or the store of the
//! included project that owns the path (the files that project uses when it is opened alone). Plugins keep one model
//! with root-relative paths and split it by owner when they read and write ([`Owners`]); the core tells them before
//! and after the owners change ([`OwnersChange`]), so nothing is written to a place that is about to move.

use std::sync::Arc;

use super::uid::ProjectUid;
use crate::paths::{key_of, WORKTREE_PREFIX};
use crate::state::StateStore;

/// One place that keeps state: the opened folder (`uid` none, `path` empty) or an included project.
#[derive(Debug, Clone)]
pub struct OwnerSlot {
    pub uid: Option<ProjectUid>,
    /// Root-relative folder (`/` separators, as the map spells it); empty for the opened folder.
    pub path: String,
    key: String,
    depth: usize,
    pub store: StateStore,
}

impl OwnerSlot {
    /// A member's slot (`path` root-relative, not empty).
    #[must_use]
    pub fn member(uid: ProjectUid, path: &str, store: StateStore) -> Self {
        Self {
            uid: Some(uid),
            key: key_of(path),
            depth: path.split('/').count(),
            path: path.to_owned(),
            store,
        }
    }

    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// The owners of an opened folder, the folder itself first. Cheap to clone (shared).
#[derive(Debug, Clone)]
pub struct Owners {
    slots: Arc<[OwnerSlot]>,
}

/// What a follower of the owners hears (`Projects::follow_owners`).
pub enum OwnersChange<'a> {
    /// The owners are about to change: write what is pending now, then hold every write until `Resume`.
    Pause,
    /// The owners now: read the state again from their files, then write as usual.
    Resume(&'a Owners),
}

fn within(key: &str, base: &str) -> bool {
    base.is_empty()
        || key == base
        || key
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl Owners {
    /// Only the opened folder (a plain folder, or a composition without projects).
    #[must_use]
    pub fn root_only(store: StateStore) -> Self {
        Self::new(store, Vec::new())
    }

    /// The opened folder's store + the members' slots (any order; nested members allowed).
    #[must_use]
    pub fn new(root: StateStore, mut members: Vec<OwnerSlot>) -> Self {
        members.retain(|m| !m.path.is_empty());
        members.sort_by(|a, b| a.key.cmp(&b.key));
        members.dedup_by(|a, b| a.key == b.key);
        let mut slots = Vec::with_capacity(members.len() + 1);
        slots.push(OwnerSlot {
            uid: None,
            path: String::new(),
            key: String::new(),
            depth: 0,
            store: root,
        });
        slots.extend(members);
        Self {
            slots: slots.into(),
        }
    }

    #[must_use]
    pub fn slots(&self) -> &[OwnerSlot] {
        &self.slots
    }

    #[must_use]
    pub fn root(&self) -> &OwnerSlot {
        &self.slots[0]
    }

    /// Only the opened folder keeps state.
    #[must_use]
    pub fn is_single(&self) -> bool {
        self.slots.len() == 1
    }

    /// The slot of a member (`None`: not an owner now).
    #[must_use]
    pub fn index_of(&self, uid: &ProjectUid) -> Option<usize> {
        self.slots.iter().position(|s| s.uid.as_ref() == Some(uid))
    }

    /// Index of the owner of a root-relative path: the deepest member holding it (its own folder included), else the
    /// opened folder (`0`; also every linked worktree path).
    #[must_use]
    pub fn owner_of(&self, path: &str) -> usize {
        if self.slots.len() == 1 || path.starts_with(WORKTREE_PREFIX) {
            return 0;
        }
        let key = key_of(path);
        let mut best = 0;
        for (i, slot) in self.slots.iter().enumerate().skip(1) {
            if within(&key, &slot.key) && slot.depth > self.slots[best].depth {
                best = i;
            }
        }
        best
    }

    /// `path` (root-relative, inside owner `i`) relative to that owner; its own folder is `""`.
    #[must_use]
    pub fn rel(&self, i: usize, path: &str) -> String {
        let depth = self.slots.get(i).map_or(0, |s| s.depth);
        if depth == 0 {
            return path.to_owned();
        }
        path.split('/').skip(depth).collect::<Vec<_>>().join("/")
    }

    /// The root-relative path of `rel` in owner `i` (`""` = the owner's folder).
    #[must_use]
    pub fn join(&self, i: usize, rel: &str) -> String {
        let base = self.slots.get(i).map_or("", |s| s.path.as_str());
        match (base.is_empty(), rel.is_empty()) {
            (true, _) => rel.to_owned(),
            (false, true) => base.to_owned(),
            (false, false) => format!("{base}/{rel}"),
        }
    }

    /// An entry of the opened folder's own file whose path lies in a member: kept as written, unused while that member
    /// is included (its own file holds the state; `projects.md` §14 D3).
    #[must_use]
    pub fn dormant(&self, path: &str) -> bool {
        self.owner_of(path) != 0
    }

    /// Same places (member ids, folders, state folders): nothing to read again.
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        self.slots.len() == other.slots.len()
            && self.slots.iter().zip(other.slots.iter()).all(|(a, b)| {
                a.uid == b.uid
                    && a.key == b.key
                    && a.store.dir() == b.store.dir()
                    && a.store.location() == b.store.location()
                    && a.store.cache_dir() == b.store.cache_dir()
            })
    }

    /// Splits root-relative items by owner: `out[i]` = the items of owner `i`, paths relative to it.
    #[must_use]
    pub fn split<T>(&self, items: impl IntoIterator<Item = (String, T)>) -> Vec<Vec<(String, T)>> {
        let mut out: Vec<Vec<(String, T)>> = (0..self.slots.len()).map(|_| Vec::new()).collect();
        for (path, item) in items {
            let i = self.owner_of(&path);
            out[i].push((self.rel(i, &path), item));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppDirs;

    fn store(base: &std::path::Path, name: &str) -> StateStore {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        StateStore::for_project(
            &AppDirs::under(base),
            &crate::paths::Root::open(&dir).unwrap(),
        )
    }

    fn uid(s: &str) -> ProjectUid {
        ProjectUid::parse(s).unwrap()
    }

    #[test]
    fn owner_split_rel_join() {
        let base = tempfile::tempdir().unwrap();
        let owners = Owners::new(
            store(base.path(), "r"),
            vec![
                OwnerSlot::member(uid("bbbbbbbbbb"), "hubs/a/sub", store(base.path(), "s")),
                OwnerSlot::member(uid("aaaaaaaaaa"), "hubs/a", store(base.path(), "a")),
                OwnerSlot::member(uid("cccccccccc"), "Ünï/e\u{301}", store(base.path(), "u")),
            ],
        );
        let a = owners.index_of(&uid("aaaaaaaaaa")).unwrap();
        let s = owners.index_of(&uid("bbbbbbbbbb")).unwrap();
        let cases: &[(&str, usize, &str)] = &[
            ("README.md", 0, "README.md"),
            ("hubs/a/x.md", a, "x.md"),
            ("hubs/a", a, ""),
            ("hubs/a/sub/y.md", s, "y.md"),
            ("hubs/a/subway/y.md", a, "subway/y.md"),
            ("hubs/ab/x.md", 0, "hubs/ab/x.md"),
            ("@wt/feat/hubs/a/x.md", 0, "@wt/feat/hubs/a/x.md"),
        ];
        for (path, want, rel) in cases {
            let i = owners.owner_of(path);
            assert_eq!(i, *want, "{path}");
            assert_eq!(owners.rel(i, path), *rel, "{path}");
            assert_eq!(owners.join(i, rel), *path, "{path}");
        }
        let u = owners.index_of(&uid("cccccccccc")).unwrap();
        assert_eq!(owners.owner_of("Ünï/e\u{301}/doc.md"), u);
        if crate::paths::CASE_INSENSITIVE {
            assert_eq!(owners.owner_of("HUBS/A/x.md"), a);
        }
        assert!(owners.dormant("hubs/a/x.md") && !owners.dormant("hubs/x.md"));
        let split = owners.split(vec![
            ("hubs/a/x.md".to_owned(), 1),
            ("top.md".to_owned(), 2),
            ("hubs/a/sub/y.md".to_owned(), 3),
        ]);
        assert_eq!(split[0], vec![("top.md".to_owned(), 2)]);
        assert_eq!(split[a], vec![("x.md".to_owned(), 1)]);
        assert_eq!(split[s], vec![("y.md".to_owned(), 3)]);
        assert!(owners.same(&owners.clone()));
        assert!(!owners.same(&Owners::root_only(store(base.path(), "r"))));
        assert!(Owners::root_only(store(base.path(), "r")).is_single());
    }
}
