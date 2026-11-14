//! The switch and the permission groups: the person's choices live in the profile only (`agent.json` per project,
//! `agent-defaults.json` for new projects); a project's `.permark/config.toml` may only take groups away.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use permark_core::fsx::file_identity;
use permark_core::state::{Loaded, StateData, StateStore};
use serde::{Deserialize, Serialize};

/// Per-project file (profile only, `PROFILE_ONLY`).
pub const PROJECT_FILE: &str = "agent.json";
/// App-wide defaults a project starts from when its switch first goes on.
pub const DEFAULTS_FILE: &str = "agent-defaults.json";

/// What an agent may do, by kind of tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Context,
    Changes,
    Find,
    CommentsRead,
    CommentsResolve,
    Notes,
    Show,
    Activity,
}

impl Group {
    pub const ALL: [Self; 8] = [
        Self::Context,
        Self::Changes,
        Self::Find,
        Self::CommentsRead,
        Self::CommentsResolve,
        Self::Notes,
        Self::Show,
        Self::Activity,
    ];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Changes => "changes",
            Self::Find => "find",
            Self::CommentsRead => "comments.read",
            Self::CommentsResolve => "comments.resolve",
            Self::Notes => "notes",
            Self::Show => "show",
            Self::Activity => "activity",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.id() == text)
    }

    /// On for a project that has not chosen: everything but resolving comments (offered once, when first asked).
    #[must_use]
    pub fn default_on(self) -> bool {
        !matches!(self, Self::CommentsResolve)
    }
}

/// `agent.json` of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChoice {
    pub enabled: bool,
    /// Group id -> on; a group not listed takes its default.
    #[serde(default)]
    pub groups: BTreeMap<String, bool>,
    /// The person was offered `comments.resolve` once already.
    #[serde(default)]
    pub resolve_asked: bool,
    /// The folder the switch was turned on for. The file is keyed by the folder's path only: a folder deleted and
    /// created | cloned again at the same path must not inherit the person's choice. Absent (written before, or the
    /// identity was unknown then) -> the path alone decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<FolderId>,
}

// `folder` is optional with a default: files written before it load as they are, so the format version stays.
impl StateData for ProjectChoice {
    const VERSION: u32 = 1;
}

/// A folder's identity on its volume (`fsx::file_identity`): kept across a rename on the same volume, new for a
/// folder created again at the same path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderId {
    pub volume: u64,
    pub file: u64,
}

impl FolderId {
    /// `None`: unknown (gone, refused, or a volume with no real ids).
    #[must_use]
    pub fn of(root: &Path) -> Option<Self> {
        file_identity(root).map(|(volume, file)| Self { volume, file })
    }
}

impl ProjectChoice {
    /// Whether agents may be answered for the folder at `root` now: the switch on, for this very folder. An unknown
    /// identity (now or when it was turned on) keeps the path-only decision.
    #[must_use]
    pub fn is_on(&self, root: &Path) -> bool {
        self.on_with(FolderId::of(root))
    }

    /// The switch was turned on for another folder that stood at this path (the app shows it off, with why).
    #[must_use]
    pub fn replaced(&self, root: &Path) -> bool {
        self.replaced_with(FolderId::of(root))
    }

    fn on_with(&self, current: Option<FolderId>) -> bool {
        self.enabled && !self.replaced_with(current)
    }

    fn replaced_with(&self, current: Option<FolderId>) -> bool {
        self.enabled && matches!((self.folder, current), (Some(then), Some(now)) if then != now)
    }
}

/// `agent-defaults.json`: the groups a project starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default)]
    pub groups: BTreeMap<String, bool>,
}

impl StateData for Defaults {
    const VERSION: u32 = 1;
}

/// Whether a group is open for this project now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    On,
    /// The person turned it off (or never turned it on).
    Off,
    /// The project's `.permark/config.toml` denies it, whatever the person chose.
    DeniedByProject,
}

/// The groups as they apply: profile choice (else default), minus what the project denies. Pure.
#[must_use]
pub fn effective(
    choice: &BTreeMap<String, bool>,
    deny: &BTreeSet<String>,
) -> BTreeMap<Group, Access> {
    Group::ALL
        .into_iter()
        .map(|group| {
            let access = if deny.contains(group.id()) {
                Access::DeniedByProject
            } else if choice
                .get(group.id())
                .copied()
                .unwrap_or_else(|| group.default_on())
            {
                Access::On
            } else {
                Access::Off
            };
            (group, access)
        })
        .collect()
}

/// The person's choices for one project, read fresh (the switch can change while an agent runs).
#[must_use]
pub fn load(store: &StateStore) -> ProjectChoice {
    match store.profile_file::<ProjectChoice>(PROJECT_FILE).load() {
        Loaded::Found(choice) => choice,
        Loaded::Missing | Loaded::Recovered { .. } => ProjectChoice::default(),
    }
}

/// The app opened the project at `root`: a switch turned on before folders were recorded gets this folder's
/// identity, once. Only the app calls it (an agent alone must not bind a folder to the person's choice); a failed
/// write is retried at the next open.
pub fn adopt_folder(store: &StateStore, root: &Path) {
    let file = store.profile_file::<ProjectChoice>(PROJECT_FILE);
    let Loaded::Found(mut choice) = file.load() else {
        return;
    };
    if !choice.enabled || choice.folder.is_some() {
        return;
    }
    let Some(folder) = FolderId::of(root) else {
        return;
    };
    choice.folder = Some(folder);
    let _ = file.save(&choice);
}

/// Whether the profile holds a choice for the project of `store` (a project PerMark has been used on).
#[must_use]
pub fn known(store: &StateStore) -> bool {
    store
        .profile_file::<ProjectChoice>(PROJECT_FILE)
        .path()
        .is_file()
}

/// The app-wide defaults (missing -> the built-in ones).
#[must_use]
pub fn defaults(store: &StateStore) -> BTreeMap<String, bool> {
    match store.app_file::<Defaults>(DEFAULTS_FILE).load() {
        Loaded::Found(defaults) => defaults.groups,
        Loaded::Missing | Loaded::Recovered { .. } => BTreeMap::new(),
    }
}

/// Only known group ids survive (a hand-edited file cannot add others).
#[must_use]
pub fn clean(groups: &BTreeMap<String, bool>) -> BTreeMap<String, bool> {
    groups
        .iter()
        .filter(|(id, _)| Group::parse(id).is_some())
        .map(|(id, on)| (id.clone(), *on))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn defaults_are_all_but_resolving_comments() {
        let groups = effective(&BTreeMap::new(), &BTreeSet::new());
        for group in Group::ALL {
            let expected = if group == Group::CommentsResolve {
                Access::Off
            } else {
                Access::On
            };
            assert_eq!(groups[&group], expected, "{}", group.id());
        }
    }

    #[test]
    fn the_project_can_only_narrow() {
        let mut choice = BTreeMap::new();
        choice.insert("comments.resolve".to_owned(), true);
        choice.insert("find".to_owned(), false);
        let groups = effective(&choice, &set(&["comments.resolve", "notes", "unknown"]));
        assert_eq!(groups[&Group::CommentsResolve], Access::DeniedByProject);
        assert_eq!(groups[&Group::Notes], Access::DeniedByProject);
        assert_eq!(
            groups[&Group::Find],
            Access::Off,
            "deny never turns a group on"
        );
        assert_eq!(groups[&Group::Context], Access::On);
    }

    #[test]
    fn group_ids_round_trip_and_unknown_ones_are_dropped() {
        for group in Group::ALL {
            assert_eq!(Group::parse(group.id()), Some(group));
        }
        assert_eq!(Group::parse("Find"), None);
        let mut raw = BTreeMap::new();
        raw.insert("find".to_owned(), true);
        raw.insert("write".to_owned(), true);
        assert_eq!(clean(&raw).len(), 1);
    }

    fn on_for(folder: Option<FolderId>) -> ProjectChoice {
        ProjectChoice {
            enabled: true,
            folder,
            ..ProjectChoice::default()
        }
    }

    #[test]
    fn the_switch_holds_only_for_the_folder_it_was_turned_on_for() {
        let a = Some(FolderId { volume: 7, file: 1 });
        let b = Some(FolderId { volume: 7, file: 2 });
        assert!(on_for(a).on_with(a));
        assert!(!on_for(a).on_with(b), "another folder at the same path");
        assert!(on_for(a).replaced_with(b));
        // an identity unknown now or then: the path alone decides, as before
        assert!(on_for(a).on_with(None));
        assert!(on_for(None).on_with(b));
        assert!(!on_for(a).replaced_with(None));
        assert!(!on_for(None).replaced_with(b));
        // off is off, never "replaced"
        let off = ProjectChoice {
            folder: a,
            ..ProjectChoice::default()
        };
        assert!(!off.on_with(a));
        assert!(!off.replaced_with(b));
    }

    #[test]
    fn a_folder_created_again_at_the_path_is_another_folder_and_a_renamed_one_is_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("proj");
        std::fs::create_dir(&first).unwrap();
        let Some(id) = FolderId::of(&first) else {
            return; // a volume without ids: nothing to compare
        };
        let choice = on_for(Some(id));
        assert!(choice.is_on(&first));
        let moved = dir.path().join("moved");
        std::fs::rename(&first, &moved).unwrap();
        // created while the old one still exists: the new folder cannot reuse its id
        std::fs::create_dir(&first).unwrap();
        assert!(!choice.is_on(&first));
        assert!(choice.replaced(&first));
        assert!(choice.is_on(&moved), "a rename keeps the identity");
        assert!(!choice.replaced(&moved));
    }

    fn store_in(dir: &Path) -> (StateStore, std::path::PathBuf) {
        let folder = dir.join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let root = permark_core::paths::Root::open(&folder).unwrap();
        let dirs = permark_core::state::AppDirs::under(&dir.join("state"));
        (
            StateStore::for_project(&dirs, &root),
            root.path().to_path_buf(),
        )
    }

    fn write_raw(store: &StateStore, json: &str) {
        let file = store.profile_file::<ProjectChoice>(PROJECT_FILE);
        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        std::fs::write(file.path(), json).unwrap();
    }

    #[test]
    fn a_grant_from_an_earlier_version_loads_and_stays_on() {
        let dir = tempfile::tempdir().unwrap();
        let (store, root) = store_in(dir.path());
        write_raw(&store, r#"{"version":1,"data":{"enabled":true}}"#);
        assert!(matches!(
            store.profile_file::<ProjectChoice>(PROJECT_FILE).load(),
            Loaded::Found(ProjectChoice { folder: None, .. })
        ));
        assert!(load(&store).is_on(&root));
        // an off choice never gains a folder by being saved again
        assert!(!serde_json::to_string(&ProjectChoice::default())
            .unwrap()
            .contains("folder"));
    }

    #[test]
    fn adopting_records_the_folder_once_and_only_for_a_switch_that_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let (store, root) = store_in(dir.path());
        let Some(id) = FolderId::of(&root) else {
            return;
        };
        adopt_folder(&store, &root);
        assert!(!known(&store), "no choice: nothing written");
        write_raw(&store, r#"{"version":1,"data":{"enabled":false}}"#);
        adopt_folder(&store, &root);
        assert_eq!(load(&store).folder, None, "off: nothing recorded");
        write_raw(&store, r#"{"version":1,"data":{"enabled":true}}"#);
        adopt_folder(&store, &root);
        assert_eq!(load(&store).folder, Some(id));
        // another folder recorded already: never written over (that one stays replaced)
        let other = FolderId {
            file: id.file ^ 1,
            ..id
        };
        store
            .profile_file::<ProjectChoice>(PROJECT_FILE)
            .save(&on_for(Some(other)))
            .unwrap();
        adopt_folder(&store, &root);
        assert_eq!(load(&store).folder, Some(other));
    }
}
