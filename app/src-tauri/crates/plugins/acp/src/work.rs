//! The work copy: the files of the project an agent may see, copied to the profile (never inside the project: a tool
//! walking up from its folder would find the project's own settings). The agent works there; a real file changes only
//! when the person accepts a change, hunk by hunk, through the same guarded writes as the rest of the app.
//!
//! Each copied file keeps a baseline: the hash of the real bytes it was copied from (at the copy, the last sync, the
//! last choice). Copy differs from baseline -> the agent changed it; real differs from baseline -> the person did.
//! Hunks are line runs over the texts with every break as `\n`, so an agent that rewrites line endings shows no
//! change, and an accepted hunk keeps the real file's own breaks and encoding.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use permark_core::document::Documents;
use permark_core::edit::{lines, LineEdit};
use permark_core::fsx::{self, ContentHash, Decoded, FsxError};
use permark_core::paths::{Root, Vault, VaultPath};
use permark_core::plugins::ProjectServices;
use permark_core::projects::Projects;
use permark_core::settings::ProjectConfig;
use permark_core::tree::Explorer;
use permark_core::watch::OwnWrites;
use permark_core::{ErrorKind, IpcError};
use permark_plugin_agent::guard::Guard;
use permark_plugin_review::{hunks, replace, Hunk};

use crate::dto::{ChangeList, FileChange, FileChangeKind, HunkView};

/// Largest file the copy takes (larger ones are skipped and counted) and the app splices.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Most files a copy holds.
pub const MAX_FILES: usize = 20_000;
/// Most bytes a copy holds.
pub const MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;
/// Most changed files one list names (the rest are counted): every file an agent deleted, listed with all its lines,
/// would freeze the window.
pub const MAX_LISTED: usize = 500;
/// Most lines one side of a hunk carries to the window; a choice still applies the whole hunk.
pub const MAX_HUNK_LINES: usize = 400;
/// Most bytes the one-step undos keep in memory (oldest dropped first).
const MAX_UNDO_BYTES: usize = 64 * 1024 * 1024;
/// Longest wait for the projects inside the folder: their rules hide paths, so no copy is made without them.
const RULES_WAIT: Duration = Duration::from_secs(5);

/// The project's own machinery (any depth): never copied, never offered back (a hook planted in `.git` would run).
const PRIVATE_FOLDERS: [&str; 2] = [".git", ".permark"];
/// Files naming what agents may not see (any depth): their lines would tell the agent the hidden names.
const PRIVATE_FILES: [&str; 1] = [".aiignore"];
/// Folders agents read as their own settings (permissions, tools, servers, hooks), any depth.
const AGENT_FOLDERS: [&str; 10] = [
    ".codex",
    ".gemini",
    ".cursor",
    ".vscode",
    ".opencode",
    ".qwen",
    ".goose",
    ".kilo",
    ".cline",
    ".amp",
];
/// Files agents read as their own settings, any depth.
const AGENT_FILES: [&str; 3] = [".mcp.json", "opencode.json", "opencode.jsonc"];
/// Agent folders that also hold instructions (documents, kept): the settings | servers | hooks inside them (files or
/// folders, right under the agent folder) stay out, and so do subagents | skills | commands: their headers grant
/// tools, a permission mode or servers once loaded.
const AGENT_SETTINGS: [(&str, &[&str]); 8] = [
    (
        ".claude",
        &[
            "settings.json",
            "settings.local.json",
            "agents",
            "skills",
            "commands",
            "hooks",
        ],
    ),
    (".kilocode", &["mcp.json"]),
    (".junie", &["mcp"]),
    (".kiro", &["settings", "hooks", "agents"]),
    (".clinerules", &["hooks"]),
    (
        ".factory",
        &["settings.json", "mcp.json", "hooks", "droids", "commands"],
    ),
    (".augment", &["settings.json"]),
    (".vibe", &["config.toml"]),
];

/// `segment` is one of `names` in any case, on every OS: the project | the copy can sit on a volume that ignores case
/// (NTFS, APFS, exFAT, a share) whatever the OS, and leaving out a harmless file costs little.
fn named(segment: &str, names: &[&str]) -> bool {
    let lower = segment.to_lowercase();
    names.iter().any(|name| lower == name.to_lowercase())
}

/// A folder whose whole content stays out of the copy.
fn kept_out_folder(name: &str) -> bool {
    named(name, &PRIVATE_FOLDERS) || named(name, &AGENT_FOLDERS)
}

/// `inside` is agent settings right under the agent folder `folder`.
fn agent_setting(folder: &str, inside: &str) -> bool {
    AGENT_SETTINGS
        .iter()
        .any(|(agent, settings)| named(folder, &[agent]) && named(inside, settings))
}

/// Whether a project path is agent settings or the project's machinery: never in the copy, never offered back.
#[must_use]
pub fn kept_out(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    let Some((name, folders)) = segments.split_last() else {
        return false;
    };
    folders.iter().any(|folder| kept_out_folder(folder))
        || named(name, &PRIVATE_FOLDERS)
        || named(name, &PRIVATE_FILES)
        || named(name, &AGENT_FILES)
        || segments
            .windows(2)
            .any(|pair| agent_setting(pair[0], pair[1]))
}

/// A folder some agent reads settings from (all of it | settings right under it): a link in its place could lead an
/// agent to settings anywhere.
fn settings_folder(name: &str) -> bool {
    kept_out_folder(name)
        || AGENT_SETTINGS
            .iter()
            .any(|(agent, _)| named(name, &[agent]))
}

/// Removes a file; a read-only one (Windows refuses to remove it) is made writable first.
fn remove_file(abs: &Path) -> io::Result<()> {
    match std::fs::remove_file(abs) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            let mut permissions = std::fs::symlink_metadata(abs)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            // a file the agent made in the copy, removed next
            permissions.set_readonly(false);
            std::fs::set_permissions(abs, permissions)?;
            std::fs::remove_file(abs)
        }
        other => other,
    }
}

/// Removes a link | special file itself, never what it leads to (Windows: a folder link goes with `remove_dir`).
fn remove_link(abs: &Path) -> io::Result<()> {
    std::fs::remove_file(abs).or_else(|_| std::fs::remove_dir(abs))
}

/// What stands at a path: the entry itself, a link never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Missing,
    Dir,
    File,
    /// A link (symlink, junction) | a special file (pipe, socket, device): reading it could lead outside | block.
    Link,
}

fn entry(abs: &Path) -> io::Result<Entry> {
    match std::fs::symlink_metadata(abs) {
        Ok(meta) => {
            let kind = meta.file_type();
            Ok(if kind.is_symlink() {
                Entry::Link
            } else if kind.is_dir() {
                Entry::Dir
            } else if kind.is_file() {
                Entry::File
            } else {
                Entry::Link
            })
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Entry::Missing),
        Err(e) => Err(e),
    }
}

/// A path of the copy as reached without following links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Every folder on the way is a real folder (or missing from there on): what stands at the path.
    Clear(Entry),
    /// A link | special file stands on the way: what lies behind it is outside the copy.
    Linked,
    /// A file stands where a folder of the path should be: nothing is at the path.
    Shadowed,
}

/// Removes what [`kept_out`] names anywhere under `dir`. A link is never followed: it is removed itself when kept out
/// or named like an agent's settings folder.
fn purge_kept_out(dir: &Path) -> io::Result<()> {
    let gone = |result: io::Result<()>| match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            // the entry itself, never what a link leads to
            let kind = entry.file_type()?;
            let abs = entry.path();
            if kind.is_symlink() {
                if kept_out(&path) || settings_folder(&name) {
                    gone(remove_link(&abs))?;
                }
            } else if kind.is_dir() {
                if kept_out(&path) || kept_out_folder(&name) {
                    gone(std::fs::remove_dir_all(&abs))?;
                } else {
                    folders.push((abs, path));
                }
            } else if kept_out(&path) {
                gone(remove_file(&abs))?;
            }
        }
    }
    Ok(())
}

/// Where the copy of `root` lives under `base`: `<base>/<project folder name>` (the agent sees the project's name).
#[must_use]
pub fn copy_dir(base: &Path, root: &Root) -> PathBuf {
    base.join(root.path().file_name().unwrap_or(OsStr::new("project")))
}

fn refusal(kind: ErrorKind, path: &str, detail: &str) -> IpcError {
    IpcError::new(kind).subject(path).detail(detail)
}

/// What the work copy needs of the open project.
#[derive(Clone)]
pub struct Project {
    vault: Vault,
    documents: Arc<Documents>,
    explorer: Arc<Explorer>,
    own: Arc<OwnWrites>,
    /// Projects inside the folder (their agent rules); `None` in compositions without them.
    projects: Option<Arc<Projects>>,
}

/// The real files a copy holds: `(project path, absolute path)`.
#[derive(Default)]
struct Listing {
    files: Vec<(String, PathBuf)>,
    bytes: u64,
    /// Files past [`MAX_FILE_BYTES`].
    skipped: usize,
}

impl Project {
    #[must_use]
    pub fn new(
        vault: Vault,
        documents: Arc<Documents>,
        explorer: Arc<Explorer>,
        own: Arc<OwnWrites>,
    ) -> Self {
        Self {
            vault,
            documents,
            explorer,
            own,
            projects: None,
        }
    }

    /// The project of a Project-scope plugin instance.
    #[must_use]
    pub fn of(vault: &Vault, services: &ProjectServices) -> Self {
        Self {
            projects: services.projects().cloned(),
            ..Self::new(
                vault.clone(),
                Arc::clone(&services.documents),
                Arc::clone(&services.explorer),
                Arc::clone(&services.own),
            )
        }
    }

    #[must_use]
    pub fn root(&self) -> &Root {
        self.vault.main()
    }

    /// What agents may not see now: `.aiignore`, `[agent] deny_paths`, the projects inside (read fresh: they can
    /// change between prompts). Fails closed while the projects inside are not known.
    fn guard(&self) -> Result<Guard, IpcError> {
        let root = self.vault.main();
        let guard = Guard::new(root, &ProjectConfig::load(root).agent_deny_paths);
        match &self.projects {
            Some(projects) => {
                let map = projects.agent_map(RULES_WAIT).ok_or_else(|| {
                    IpcError::new(ErrorKind::Unavailable).detail("projectsNotReady")
                })?;
                Ok(guard.with_projects(map, None))
            }
            None => Ok(guard),
        }
    }

    /// The real files an agent may see: the tree's walk (its ignore rules), minus hidden paths and agent settings,
    /// minus files past the size cap.
    ///
    /// # Errors
    /// `TooLarge` (`scanCutOff` | `tooManyFiles` | `tooManyBytes`): a partial copy would show the agent a project with
    /// files missing.
    fn listing(&self) -> Result<Listing, IpcError> {
        let guard = self.guard()?;
        let cancel = AtomicBool::new(false);
        let scan = self.explorer.scan_folder(
            &VaultPath::root(),
            self.vault.main().path(),
            &cancel,
            &mut |_| false,
        )?;
        if scan.cut_off {
            return Err(IpcError::new(ErrorKind::TooLarge).detail("scanCutOff"));
        }
        let mut listing = Listing::default();
        for file in scan.files {
            if kept_out(&file.path) || guard.hidden(&file.path, false) {
                continue;
            }
            let Some(abs) = file.abs else {
                continue;
            };
            if file.size > MAX_FILE_BYTES {
                listing.skipped += 1;
                continue;
            }
            listing.bytes = listing.bytes.saturating_add(file.size);
            listing.files.push((file.path, abs));
        }
        if listing.files.len() > MAX_FILES {
            return Err(IpcError::new(ErrorKind::TooLarge).detail("tooManyFiles"));
        }
        if listing.bytes > MAX_TOTAL_BYTES {
            return Err(IpcError::new(ErrorKind::TooLarge).detail("tooManyBytes"));
        }
        Ok(listing)
    }

    /// Whether agents may not see the project path `path` now (`.aiignore`, `[agent] deny_paths`, private projects
    /// inside, `.git`, `.permark`): the copy leaves it out, and nothing of it may reach an agent another way.
    ///
    /// # Errors
    /// `InvalidPath`: no project path; `Unavailable` (`projectsNotReady`).
    pub fn hidden(&self, path: &str) -> Result<bool, IpcError> {
        let path = VaultPath::parse(path)?;
        Ok(self.guard()?.hidden(path.as_str(), false))
    }

    /// A window holds unsaved edits of `path`: its file is not written under it.
    fn unsaved(&self, path: &str) -> bool {
        VaultPath::parse(path).is_ok_and(|p| self.documents.unsaved_keys().contains(&p.key()))
    }

    /// The real file of a project path, checked against the folder's boundary.
    fn resolve(&self, path: &str) -> Result<PathBuf, IpcError> {
        Ok(self.vault.resolve(&VaultPath::parse(path)?)?)
    }

    /// The real file an accept | an undo may write | remove: neither the path as sent nor the place the folder resolves
    /// it to (an 8.3 short name such as `GIT~1`, another spelling, a link) may be kept out | hidden.
    ///
    /// # Errors
    /// `InvalidPath` | `PathEscape`; `Denied` (`denied`); `Unavailable` (`projectsNotReady`).
    fn destination(&self, path: &str) -> Result<PathBuf, IpcError> {
        let parsed = VaultPath::parse(path)?;
        let abs = self.vault.resolve(&parsed)?;
        let denied = || refusal(ErrorKind::Denied, path, "denied");
        let real = self.vault.relativize(&abs).ok_or_else(denied)?;
        let guard = self.guard()?;
        if [parsed.as_str(), real.as_str()]
            .iter()
            .any(|p| kept_out(p) || guard.hidden(p, false))
        {
            return Err(denied());
        }
        Ok(abs)
    }
}

/// A file's state as last seen: what a whole-file choice | an undo replaces only while it is still there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    Missing,
    Bytes(ContentHash),
}

impl Seen {
    /// The guard of the write over it: these bytes, or nothing there.
    fn hash(self) -> Option<ContentHash> {
        match self {
            Self::Missing => None,
            Self::Bytes(hash) => Some(hash),
        }
    }
}

/// One side of a file now.
enum Side {
    Missing,
    /// Past [`MAX_FILE_BYTES`]: changed for sure (every copied file was smaller), never read.
    Big,
    Bytes(Vec<u8>),
    /// A link | special file in the copy: never opened.
    Link,
}

impl Side {
    /// The real file (the project's own links are the person's: the vault resolved them inside the folder).
    fn real(abs: &Path) -> Result<Self, IpcError> {
        if abs.is_dir() {
            return Ok(Self::Missing);
        }
        Self::file(abs)
    }

    fn file(abs: &Path) -> Result<Self, IpcError> {
        match fsx::read_bytes(abs, MAX_FILE_BYTES) {
            Ok(bytes) => Ok(Self::Bytes(bytes)),
            Err(FsxError::TooLarge { .. }) => Ok(Self::Big),
            Err(FsxError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(Self::Missing)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn hash(&self) -> Option<ContentHash> {
        match self {
            Self::Bytes(bytes) => Some(fsx::hash(bytes)),
            Self::Missing | Self::Big | Self::Link => None,
        }
    }

    /// Text the app can splice back byte for byte (decoded without loss).
    fn text(&self) -> Option<Decoded> {
        match self {
            Self::Bytes(bytes) => fsx::decode(bytes).filter(|d| !d.lossy),
            Self::Missing | Self::Big | Self::Link => None,
        }
    }

    /// What a whole-file choice | an undo compares; `None`: past the cap | a link (cannot be compared).
    fn seen(&self) -> Option<Seen> {
        match self {
            Self::Missing => Some(Seen::Missing),
            Self::Bytes(bytes) => Some(Seen::Bytes(fsx::hash(bytes))),
            Self::Big | Self::Link => None,
        }
    }

    /// What an undo would put back (`None`: past the cap | a link, not kept).
    fn kept(&self) -> Option<Held> {
        match self {
            Self::Missing => Some(Held::Nothing),
            Self::Bytes(bytes) => Some(Held::Bytes(bytes.clone())),
            Self::Big | Self::Link => None,
        }
    }

    fn bytes(&self, path: &str) -> Result<&[u8], IpcError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::Big => Err(IpcError::new(ErrorKind::TooLarge).subject(path)),
            Self::Missing => Err(IpcError::new(ErrorKind::NotFound).subject(path)),
            Self::Link => Err(refusal(ErrorKind::InvalidData, path, "link")),
        }
    }
}

/// Lines of a normalized text (`None`: no file, no lines).
fn lines_of(text: Option<&str>) -> Vec<&str> {
    text.map_or_else(Vec::new, |t| t.split('\n').collect())
}

/// The key of a hunk: its file, place and the lines of both sides, each part length-prefixed (two hunks of one list
/// never share one; a hunk that moved | changed gets another).
fn hunk_key(path: &str, hunk: &Hunk, old: &[&str], new: &[&str]) -> String {
    let mut bytes = Vec::new();
    let mut part = |data: &[u8]| {
        let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(data);
    };
    part(path.as_bytes());
    part(
        &u64::try_from(hunk.old.start)
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for (lines, range) in [(old, &hunk.old), (new, &hunk.new)] {
        let run = lines.get(range.clone()).unwrap_or_default();
        part(&u64::try_from(run.len()).unwrap_or(u64::MAX).to_le_bytes());
        for line in run {
            part(line.as_bytes());
        }
    }
    fsx::hash(&bytes).hex()
}

/// One file's difference now, with what a choice needs.
struct Look {
    kind: FileChangeKind,
    real: Side,
    copy: Side,
    real_text: Option<Decoded>,
    copy_text: Option<Decoded>,
    /// Normalized real text (the old side of the hunks; `None`: no lines).
    old: Option<String>,
    /// Normalized copy text (the new side).
    new: Option<String>,
    hunks: Vec<Hunk>,
}

impl Look {
    fn keys(&self, path: &str) -> Vec<String> {
        let old = lines_of(self.old.as_deref());
        let new = lines_of(self.new.as_deref());
        self.hunks
            .iter()
            .map(|hunk| hunk_key(path, hunk, &old, &new))
            .collect()
    }

    fn view(&self, path: &str) -> FileChange {
        let old = lines_of(self.old.as_deref());
        let new = lines_of(self.new.as_deref());
        // the first lines only: the window shows a long run cut, a choice applies all of it
        let shown = |lines: &[&str], range: &std::ops::Range<usize>| -> (Vec<String>, bool) {
            let run = lines.get(range.clone()).unwrap_or_default();
            let first = run.iter().take(MAX_HUNK_LINES).map(|l| (*l).to_owned());
            (first.collect(), run.len() > MAX_HUNK_LINES)
        };
        // a text ending in a line break splits into a last empty piece that is no line of the file
        let real_lines = old.len() - usize::from(old.last() == Some(&""));
        let context = |at: Option<usize>| {
            at.filter(|i| *i < real_lines)
                .and_then(|i| old.get(i))
                .map(|l| (*l).to_owned())
        };
        FileChange {
            path: path.to_owned(),
            kind: self.kind,
            hunks: self
                .hunks
                .iter()
                .map(|hunk| {
                    let (old_lines, old_cut) = shown(&old, &hunk.old);
                    let (new_lines, new_cut) = shown(&new, &hunk.new);
                    HunkView {
                        key: hunk_key(path, hunk, &old, &new),
                        old_start: u32::try_from(hunk.old.start + 1).unwrap_or(u32::MAX),
                        old_lines,
                        new_lines,
                        truncated: old_cut || new_cut,
                        context_before: context(hunk.old.start.checked_sub(1)),
                        context_after: context(Some(hunk.old.end)),
                    }
                })
                .collect(),
        }
    }
}

/// The hunks `keys` name, as sorted indexes (`None`: every hunk).
///
/// # Errors
/// `InvalidData` (`unknownHunk`): no key; `Conflict` (`outdated`): a key of an older list.
fn pick(keys: Option<&[String]>, look: &Look, path: &str) -> Result<Vec<usize>, IpcError> {
    let Some(keys) = keys else {
        return Ok((0..look.hunks.len()).collect());
    };
    if keys.is_empty() {
        return Err(refusal(ErrorKind::InvalidData, path, "unknownHunk"));
    }
    let fresh = look.keys(path);
    let picked = keys
        .iter()
        .map(|key| {
            fresh
                .iter()
                .position(|k| k == key)
                .ok_or_else(|| refusal(ErrorKind::Conflict, path, "outdated"))
        })
        .collect::<Result<BTreeSet<usize>, IpcError>>()?;
    Ok(picked.into_iter().collect())
}

/// A file only chosen as a whole: keys allowed only when they name every hunk.
fn whole_only(keys: Option<&[String]>, look: &Look, path: &str) -> Result<(), IpcError> {
    match keys {
        Some(keys) if pick(Some(keys), look, path)?.len() != look.hunks.len() => {
            Err(refusal(ErrorKind::InvalidData, path, "wholeFileOnly"))
        }
        _ => Ok(()),
    }
}

fn write_copy(abs: &Path, bytes: &[u8], path: &str) -> Result<(), IpcError> {
    fsx::write_atomic(abs, bytes).map_err(|e| IpcError::io(path, &e))
}

fn link_in_path(path: &str) -> IpcError {
    refusal(ErrorKind::Conflict, path, "linkInPath")
}

/// Where a choice wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The real file (an accept).
    Real,
    /// The copy's file (a reject).
    Copy,
}

/// What a file held before a choice.
enum Held {
    Nothing,
    Bytes(Vec<u8>),
}

/// What one accept | reject replaced, to be put back once.
struct Undo {
    path: String,
    place: Place,
    before: Held,
    /// What the choice left there: the undo applies only while it is still there.
    after: Seen,
    /// The file's baseline before the choice.
    baseline: Option<ContentHash>,
}

impl Undo {
    fn size(&self) -> usize {
        match &self.before {
            Held::Nothing => 0,
            Held::Bytes(bytes) => bytes.len(),
        }
    }
}

/// The copy of one project and what it started from.
pub struct WorkCopy {
    project: Project,
    dir: PathBuf,
    /// Project path -> hash of the real bytes the copy holds as unchanged.
    files: BTreeMap<String, ContentHash>,
    /// Real state of each `binary` | `conflict` file when last listed: a whole-file accept replaces
    /// only what the person saw.
    reviewed: HashMap<String, Seen>,
    skipped: usize,
    /// The change list as last computed (reading both sides of every file is costly), kept for the window; each
    /// choice updates its own file in it.
    listed: Option<ChangeList>,
    /// One step back per file, oldest first: what its last accept | reject replaced.
    undos: Vec<Undo>,
}

impl WorkCopy {
    /// Copies what an agent may see of `project` into `dir` (a copy left there before is replaced).
    ///
    /// # Errors
    /// `InvalidPath`: `dir` inside the project | the project inside `dir`; the listing's refusals; IO.
    pub fn build(project: Project, dir: PathBuf) -> Result<Self, IpcError> {
        let root = project.root().path();
        if dir.starts_with(root) || root.starts_with(&dir) {
            return Err(IpcError::new(ErrorKind::InvalidPath)
                .subject(dir.display().to_string())
                .detail("insideProject"));
        }
        let listing = project.listing()?;
        let mut copy = Self {
            project,
            dir,
            files: BTreeMap::new(),
            reviewed: HashMap::new(),
            skipped: listing.skipped,
            listed: None,
            undos: Vec::new(),
        };
        let filled = copy.clear().and_then(|()| copy.fill(listing.files));
        if filled.is_err() {
            // a half-made copy would show the agent a project with files missing
            let _ = copy.clear();
        }
        filled.map(|()| copy)
    }

    fn clear(&self) -> Result<(), IpcError> {
        match std::fs::remove_dir_all(&self.dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(IpcError::io(self.dir.display().to_string(), &e))
            }
            _ => Ok(()),
        }
    }

    fn fill(&mut self, files: Vec<(String, PathBuf)>) -> Result<(), IpcError> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| IpcError::io(self.dir.display().to_string(), &e))?;
        for (path, abs) in files {
            // the bytes read once: the baseline is exactly what the copy holds
            let Ok(bytes) = fsx::read_bytes(&abs, MAX_FILE_BYTES) else {
                // grew past the cap | went since the walk
                self.skipped += 1;
                continue;
            };
            // a folder just made, before any agent ran in it: no link can stand on the way
            write_copy(&self.copy_path(&path)?, &bytes, &path)?;
            self.files.insert(path, fsx::hash(&bytes));
        }
        Ok(())
    }

    /// The copy's folder (where the agent works).
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Removes agent settings and project machinery found in the copy (before each agent start and each prompt).
    /// They are never copied from the project, so the agent made them: settings it wrote would load at its next start
    /// and could allow anything without a question. Links are removed themselves, never followed.
    ///
    /// # Errors
    /// IO (the agent must not start on a copy that still holds them).
    pub fn purge(&self) -> Result<(), IpcError> {
        purge_kept_out(&self.dir).map_err(|e| IpcError::io(self.dir.display().to_string(), &e))
    }

    /// Files left out for their size at the last build | sync.
    #[must_use]
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// The copy of a project path: always inside the copy (a path climbing out is refused). Names the place only:
    /// reads | writes go through [`Self::reach`], which never follows a link.
    ///
    /// # Errors
    /// `InvalidPath` | `PathEscape`: not a file path of the project.
    pub fn copy_path(&self, path: &str) -> Result<PathBuf, IpcError> {
        let parsed = VaultPath::parse(path)?;
        if parsed.is_root() {
            return Err(IpcError::new(ErrorKind::InvalidPath).subject(path));
        }
        let mut abs = self.dir.clone();
        abs.extend(parsed.segments());
        Ok(abs)
    }

    /// Where `path` is in the copy and what stands there, reached without following links: the copy's own folder and
    /// each folder on the way must be real folders (the agent can put a link anywhere in the copy).
    fn reach(&self, path: &str) -> Result<(PathBuf, Reach), IpcError> {
        let abs = self.copy_path(path)?;
        let io = |e: io::Error| IpcError::io(path, &e);
        let mut at = self.dir.clone();
        let mut found = entry(&at).map_err(io)?;
        for segment in VaultPath::parse(path)?.segments() {
            match found {
                Entry::Dir => {}
                Entry::Missing => return Ok((abs, Reach::Clear(Entry::Missing))),
                Entry::File => return Ok((abs, Reach::Shadowed)),
                Entry::Link => return Ok((abs, Reach::Linked)),
            }
            at.push(segment);
            found = entry(&at).map_err(io)?;
        }
        Ok((abs, Reach::Clear(found)))
    }

    /// One file of the copy now: a link | special file there is never opened; a file behind a link on the way counts
    /// as missing (it is outside the copy).
    fn read_copy(&self, path: &str) -> Result<Side, IpcError> {
        match self.reach(path)? {
            (abs, Reach::Clear(Entry::File)) => Side::file(&abs),
            (_, Reach::Clear(Entry::Link)) => Ok(Side::Link),
            (_, Reach::Clear(Entry::Missing | Entry::Dir) | Reach::Linked | Reach::Shadowed) => {
                Ok(Side::Missing)
            }
        }
    }

    /// Writes a file of the copy; refused where a link stands on the way | at the path (the write would land outside).
    fn put_copy(&self, path: &str, bytes: &[u8]) -> Result<(), IpcError> {
        match self.reach(path)? {
            (_, Reach::Clear(Entry::Link) | Reach::Linked) => Err(link_in_path(path)),
            (abs, Reach::Clear(_) | Reach::Shadowed) => write_copy(&abs, bytes, path),
        }
    }

    /// Removes a file of the copy (a link | special file there goes itself); refused where a link stands on the way.
    fn drop_copy(&self, path: &str) -> Result<(), IpcError> {
        let gone = |result: io::Result<()>| match result {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(IpcError::io(path, &e)),
            _ => Ok(()),
        };
        match self.reach(path)? {
            (abs, Reach::Clear(Entry::File)) => gone(remove_file(&abs)),
            (abs, Reach::Clear(Entry::Link)) => gone(remove_link(&abs)),
            (_, Reach::Clear(Entry::Missing | Entry::Dir) | Reach::Shadowed) => Ok(()),
            (_, Reach::Linked) => Err(link_in_path(path)),
        }
    }

    /// Brings the person's changes into the copy (before each prompt): a real file changed where the copy is
    /// unchanged replaces the copy; new real files join it; a real file gone (or now hidden) leaves an unchanged copy.
    /// Where both changed, the agent's version stays (listed as a conflict). Nothing goes behind a link of the copy.
    ///
    /// # Errors
    /// The listing's refusals; IO.
    pub fn sync(&mut self) -> Result<(), IpcError> {
        let listing = self.project.listing()?;
        self.skipped = listing.skipped;
        let mut listed = HashSet::with_capacity(listing.files.len());
        for (path, abs) in listing.files {
            listed.insert(path.clone());
            let Ok(bytes) = fsx::read_bytes(&abs, MAX_FILE_BYTES) else {
                continue;
            };
            let real = fsx::hash(&bytes);
            match self.files.get(&path).copied() {
                Some(base) if base == real => {}
                Some(base) => {
                    if self.read_copy(&path)?.hash() == Some(base) {
                        self.put_copy(&path, &bytes)?;
                        self.files.insert(path, real);
                    }
                }
                // a file the agent made at that path stays (listed as added), and so does a link on the way
                None => {
                    if self.reach(&path)?.1 == Reach::Clear(Entry::Missing) {
                        self.put_copy(&path, &bytes)?;
                        self.files.insert(path, real);
                    }
                }
            }
        }
        let gone: Vec<(String, ContentHash)> = self
            .files
            .iter()
            .filter(|(path, _)| !listed.contains(*path))
            .map(|(path, base)| (path.clone(), *base))
            .collect();
        for (path, base) in gone {
            match self.read_copy(&path)? {
                Side::Bytes(bytes) if fsx::hash(&bytes) == base => {
                    self.drop_copy(&path)?;
                    self.files.remove(&path);
                }
                Side::Missing => {
                    self.files.remove(&path);
                }
                // changed in the copy: listed as a conflict | a link
                Side::Bytes(_) | Side::Big | Side::Link => {}
            }
        }
        Ok(())
    }

    /// Files of the copy (the ignore files it holds apply; agent settings and machinery left out), links and special
    /// files as themselves (never followed: listed so the person can remove them).
    fn copy_files(&self) -> Result<Vec<String>, IpcError> {
        let walk = ignore::WalkBuilder::new(&self.dir)
            .standard_filters(false)
            .hidden(false)
            .parents(false)
            .ignore(true)
            .git_ignore(true)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !entry.file_type().is_some_and(|t| t.is_dir())
                    || !entry.file_name().to_str().is_some_and(kept_out_folder)
            })
            .build();
        let mut out = Vec::new();
        for entry in walk.flatten() {
            if entry.file_type().is_none_or(|t| t.is_dir()) {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.dir) else {
                continue;
            };
            let Some(text) = rel.to_str() else {
                continue;
            };
            let Ok(path) = VaultPath::parse(text) else {
                continue;
            };
            if path.is_root() || kept_out(path.as_str()) {
                continue;
            }
            out.push(path.as_str().to_owned());
            if out.len() > MAX_FILES.saturating_mul(2) {
                return Err(IpcError::new(ErrorKind::TooLarge).detail("tooManyFiles"));
            }
        }
        Ok(out)
    }

    /// The difference of one file now (`None`: no change to offer).
    fn look(&self, path: &str) -> Result<Option<Look>, IpcError> {
        let real = Side::real(&self.project.resolve(path)?)?;
        let copy = self.read_copy(path)?;
        let baseline = self.files.get(path).copied();
        let kind = match (baseline, &copy) {
            (_, Side::Link) => FileChangeKind::Link,
            (None, Side::Missing) => return Ok(None),
            (None, _) => FileChangeKind::Added,
            (Some(base), Side::Bytes(bytes)) if fsx::hash(bytes) == base => return Ok(None),
            (Some(base), Side::Missing) => match &real {
                Side::Missing => return Ok(None),
                _ if real.hash() == Some(base) => FileChangeKind::Deleted,
                _ => FileChangeKind::Conflict,
            },
            (Some(base), _) if real.hash() != Some(base) => FileChangeKind::Conflict,
            (Some(_), _) => FileChangeKind::Modified,
        };
        let real_text = real.text();
        let copy_text = copy.text();
        let normal = |text: &Option<Decoded>| text.as_ref().map(|d| lines::normalized(&d.text));
        let (kind, old, new) = match kind {
            FileChangeKind::Modified if real_text.is_none() || copy_text.is_none() => {
                (FileChangeKind::Binary, None, None)
            }
            FileChangeKind::Modified => (kind, normal(&real_text), normal(&copy_text)),
            FileChangeKind::Added => (kind, None, normal(&copy_text)),
            FileChangeKind::Deleted => (kind, normal(&real_text), None),
            FileChangeKind::Binary | FileChangeKind::Conflict | FileChangeKind::Link => {
                (kind, None, None)
            }
        };
        let lined = old.is_some() || new.is_some();
        let found = if lined {
            hunks(&lines_of(old.as_deref()), &lines_of(new.as_deref()))
        } else {
            Vec::new()
        };
        // the same text with other line breaks | encoding: the real file keeps its own
        if kind == FileChangeKind::Modified && found.is_empty() {
            return Ok(None);
        }
        Ok(Some(Look {
            kind,
            real,
            copy,
            real_text,
            copy_text,
            old,
            new,
            hunks: found,
        }))
    }

    /// Keeps the real state of a whole-file change as listed: its accept replaces only that.
    fn review(&mut self, path: &str, look: Option<&Look>) {
        let whole =
            look.filter(|l| matches!(l.kind, FileChangeKind::Binary | FileChangeKind::Conflict));
        match whole.and_then(|l| l.real.seen()) {
            Some(state) => {
                self.reviewed.insert(path.to_owned(), state);
            }
            None => {
                self.reviewed.remove(path);
            }
        }
    }

    /// The kept change list (computed when none is kept yet).
    ///
    /// # Errors
    /// See [`Self::refresh`].
    pub fn changes(&mut self) -> Result<ChangeList, IpcError> {
        match &self.listed {
            Some(list) => Ok(list.clone()),
            None => self.refresh(),
        }
    }

    /// Computes the change list again (every file of both sides, by path; the first [`MAX_LISTED`] listed, the rest
    /// counted) and keeps it.
    ///
    /// # Errors
    /// A file of either side cannot be read; the copy holds far too many files.
    pub fn refresh(&mut self) -> Result<ChangeList, IpcError> {
        let mut paths: BTreeSet<String> = self.files.keys().cloned().collect();
        paths.extend(self.copy_files()?);
        self.reviewed.clear();
        let mut list = ChangeList::default();
        for path in paths {
            let Some(look) = self.look(&path)? else {
                continue;
            };
            self.review(&path, Some(&look));
            if list.files.len() < MAX_LISTED {
                list.files.push(look.view(&path));
            } else {
                list.more = list.more.saturating_add(1);
            }
        }
        self.listed = Some(list.clone());
        Ok(list)
    }

    /// Looks at `path` again and updates it in the kept list (computed whole while none is kept | files are left
    /// unlisted past the cap: one of them may move up).
    fn patch(&mut self, path: &str) -> Result<ChangeList, IpcError> {
        // a path named over IPC that the full list never offers (agent settings, machinery) stays out of it
        let look = if kept_out(path) {
            None
        } else {
            self.look(path)?
        };
        self.review(path, look.as_ref());
        let view = look.map(|l| l.view(path));
        let placed = match self.listed.as_mut() {
            Some(list) if list.more == 0 => {
                let at = list.files.binary_search_by(|f| f.path.as_str().cmp(path));
                match (at, view) {
                    (Ok(at), Some(view)) => {
                        list.files.get_mut(at).map(|slot| *slot = view).is_some()
                    }
                    (Ok(at), None) => {
                        list.files.remove(at);
                        true
                    }
                    (Err(at), Some(view)) if list.files.len() < MAX_LISTED => {
                        list.files.insert(at, view);
                        true
                    }
                    (Err(_), Some(_)) => false,
                    (Err(_), None) => true,
                }
            }
            _ => false,
        };
        match (&self.listed, placed) {
            (Some(list), true) => Ok(list.clone()),
            _ => self.refresh(),
        }
    }

    /// After a choice (refused or not): the kept list with `path` looked at again; a refusal's list is read by the
    /// window next (`acp.changes`), so it shows what the refusal was about.
    fn answer(&mut self, path: &str, done: Result<(), IpcError>) -> Result<ChangeList, IpcError> {
        match done {
            Ok(()) => self.patch(path),
            Err(error) => {
                let _ = self.patch(path);
                Err(error)
            }
        }
    }

    /// Keeps `undo` as the one step back of its file (the oldest go past [`MAX_UNDO_BYTES`]).
    fn keep_undo(&mut self, undo: Undo) {
        self.forget_undo(&undo.path);
        self.undos.push(undo);
        let mut kept: usize = self.undos.iter().map(Undo::size).sum();
        while kept > MAX_UNDO_BYTES && !self.undos.is_empty() {
            kept = kept.saturating_sub(self.undos.remove(0).size());
        }
    }

    fn forget_undo(&mut self, path: &str) {
        self.undos.retain(|u| u.path != path);
    }

    /// Writes `bytes` over the real file while it still holds `seen` (`None`: only where nothing is), announced as
    /// the app's own write.
    fn write_real(
        &self,
        path: &str,
        abs: &Path,
        bytes: &[u8],
        seen: Option<ContentHash>,
    ) -> Result<ContentHash, IpcError> {
        let new = fsx::hash(bytes);
        self.project.own.expect_write(path, new);
        let written = match seen {
            Some(seen) => fsx::write_atomic_if_unchanged(abs, bytes, seen),
            None => fsx::write_atomic_if_absent(abs, bytes),
        };
        match written {
            Ok(true) => {
                if seen.is_none() {
                    self.project
                        .explorer
                        .sync_paths(std::slice::from_ref(&path.to_owned()), &[]);
                }
                Ok(new)
            }
            Ok(false) => {
                self.project.own.retract_write(path, new);
                Err(if seen.is_some() {
                    refusal(ErrorKind::Conflict, path, "changed")
                } else {
                    refusal(ErrorKind::Exists, path, "exists")
                })
            }
            Err(error) => {
                self.project.own.retract_write(path, new);
                Err(IpcError::io(path, &error))
            }
        }
    }

    /// Sends the real file to the recycle bin.
    fn recycle_real(&self, path: &str, abs: &Path) -> Result<(), IpcError> {
        let paths = [path.to_owned()];
        self.project.own.expect_removals(&paths);
        if let Err(reason) = fsx::recycle(&[abs.to_path_buf()]) {
            self.project.own.retract_moves(&paths);
            return Err(IpcError::new(ErrorKind::Trash).subject(path).detail(reason));
        }
        self.project.explorer.sync_paths(&paths, &[]);
        Ok(())
    }

    /// Applies the agent's change to the real file: the hunks `keys` name (`None`: all), or the whole file for the
    /// kinds that have no hunks. Refused for a path kept out | hidden (as sent | as the folder resolves it), while a
    /// window holds unsaved edits of the file, and when the real file changed since the copy was taken (whole file:
    /// since it was listed). What it replaced is kept for [`Self::undo`]. Answers the updated list.
    ///
    /// # Errors
    /// `Denied` (`denied`), `NotFound` (`noChange`), `Conflict` (`unsaved` | `changed` | `outdated`), `Exists` (a new
    /// file where one appeared), `InvalidData` (`unknownHunk` | `wholeFileOnly` | `link`), `TooLarge`, `Trash`, IO.
    pub fn accept(&mut self, path: &str, keys: Option<&[String]>) -> Result<ChangeList, IpcError> {
        let done = self.apply(path, keys);
        self.answer(path, done)
    }

    fn apply(&mut self, path: &str, keys: Option<&[String]>) -> Result<(), IpcError> {
        // first: nothing is read for a path that may never be written
        let abs = self.project.destination(path)?;
        let look = self
            .look(path)?
            .ok_or_else(|| refusal(ErrorKind::NotFound, path, "noChange"))?;
        if self.project.unsaved(path) {
            return Err(refusal(ErrorKind::Conflict, path, "unsaved"));
        }
        let baseline = self.files.get(path).copied();
        let after = match look.kind {
            FileChangeKind::Modified => {
                let picked = pick(keys, &look, path)?;
                let (Some(real), Some(new)) = (&look.real_text, &look.new) else {
                    return Err(refusal(ErrorKind::Internal, path, "noText"));
                };
                let source = lines_of(Some(new));
                let edits: Vec<LineEdit> = picked
                    .iter()
                    .filter_map(|at| look.hunks.get(*at))
                    .map(|hunk| replace(&hunk.old, &source, &hunk.new))
                    .collect();
                let text = lines::apply(&real.text, &edits)
                    .map_err(|_| refusal(ErrorKind::Internal, path, "edits"))?;
                let encoded = lines::encode(&text, real.encoding);
                let hash = self.write_real(path, &abs, &encoded, baseline)?;
                self.files.insert(path.to_owned(), hash);
                Seen::Bytes(hash)
            }
            FileChangeKind::Added => {
                whole_only(keys, &look, path)?;
                let hash = self.write_real(path, &abs, look.copy.bytes(path)?, None)?;
                self.files.insert(path.to_owned(), hash);
                Seen::Bytes(hash)
            }
            FileChangeKind::Deleted => {
                whole_only(keys, &look, path)?;
                self.recycle_real(path, &abs)?;
                self.files.remove(path);
                Seen::Missing
            }
            FileChangeKind::Binary => {
                whole_only(keys, &look, path)?;
                let hash = self.write_real(path, &abs, look.copy.bytes(path)?, baseline)?;
                self.files.insert(path.to_owned(), hash);
                Seen::Bytes(hash)
            }
            FileChangeKind::Conflict => {
                if keys.is_some() {
                    return Err(refusal(ErrorKind::Conflict, path, "changed"));
                }
                let now = look
                    .real
                    .seen()
                    .ok_or_else(|| IpcError::new(ErrorKind::TooLarge).subject(path))?;
                if self.reviewed.get(path) != Some(&now) {
                    return Err(refusal(ErrorKind::Conflict, path, "changed"));
                }
                if let Side::Missing = look.copy {
                    self.recycle_real(path, &abs)?;
                    self.files.remove(path);
                    Seen::Missing
                } else {
                    let hash = self.write_real(path, &abs, look.copy.bytes(path)?, now.hash())?;
                    self.files.insert(path.to_owned(), hash);
                    Seen::Bytes(hash)
                }
            }
            // what a link leads to is never offered as content
            FileChangeKind::Link => return Err(refusal(ErrorKind::InvalidData, path, "link")),
        };
        if let Some(before) = look.real.kept() {
            self.keep_undo(Undo {
                path: path.to_owned(),
                place: Place::Real,
                before,
                after,
                baseline,
            });
        }
        self.settle(path)
    }

    /// Drops the agent's change in the copy: the hunks `keys` name of a modified file get the real lines back; a link
    /// goes (a file the copy had there gets the real one back); anything else (`None`: the whole file) takes the real
    /// file as it is now. What it replaced is kept for [`Self::undo`] (a link: never put back). Answers the updated
    /// list.
    ///
    /// # Errors
    /// `NotFound` (`noChange`), `Conflict` (`outdated` | `linkInPath`), `InvalidData` (`unknownHunk` |
    /// `wholeFileOnly`), IO.
    pub fn reject(&mut self, path: &str, keys: Option<&[String]>) -> Result<ChangeList, IpcError> {
        let done = self.discard(path, keys);
        self.answer(path, done)
    }

    fn discard(&mut self, path: &str, keys: Option<&[String]>) -> Result<(), IpcError> {
        let look = self
            .look(path)?
            .ok_or_else(|| refusal(ErrorKind::NotFound, path, "noChange"))?;
        let baseline = self.files.get(path).copied();
        match look.kind {
            FileChangeKind::Modified if keys.is_some() => {
                let picked = pick(keys, &look, path)?;
                let (Some(copy), Some(old)) = (&look.copy_text, &look.old) else {
                    return Err(refusal(ErrorKind::Internal, path, "noText"));
                };
                let source = lines_of(Some(old));
                let edits: Vec<LineEdit> = picked
                    .iter()
                    .filter_map(|at| look.hunks.get(*at))
                    .map(|hunk| replace(&hunk.new, &source, &hunk.old))
                    .collect();
                let text = lines::apply(&copy.text, &edits)
                    .map_err(|_| refusal(ErrorKind::Internal, path, "edits"))?;
                self.put_copy(path, &lines::encode(&text, copy.encoding))?;
            }
            FileChangeKind::Added => {
                whole_only(keys, &look, path)?;
                self.drop_copy(path)?;
            }
            FileChangeKind::Link => {
                whole_only(keys, &look, path)?;
                self.drop_copy(path)?;
                // only a file the copy held: a real file never copied (hidden) stays out
                if baseline.is_some() {
                    self.take_real(path, &look.real)?;
                }
            }
            _ => {
                whole_only(keys, &look, path)?;
                self.take_real(path, &look.real)?;
            }
        }
        self.settle(path)?;
        let after = self.read_copy(path)?.seen();
        match (look.copy.kept(), after) {
            (Some(before), Some(after)) if look.kind != FileChangeKind::Link => {
                self.keep_undo(Undo {
                    path: path.to_owned(),
                    place: Place::Copy,
                    before,
                    after,
                    baseline,
                });
            }
            _ => self.forget_undo(path),
        }
        Ok(())
    }

    /// The copy takes the real file as it is now (gone | past the cap: out of the copy, no baseline).
    fn take_real(&mut self, path: &str, real: &Side) -> Result<(), IpcError> {
        match real {
            Side::Bytes(bytes) => {
                self.put_copy(path, bytes)?;
                self.files.insert(path.to_owned(), fsx::hash(bytes));
            }
            Side::Missing | Side::Big | Side::Link => {
                self.drop_copy(path)?;
                self.files.remove(path);
            }
        }
        Ok(())
    }

    /// Takes back the last accept | reject of `path`, once: the real file (accept: its earlier bytes written back, a
    /// deleted one made again, an added one recycled) | the copy's file (reject) gets back what it held, only while
    /// it still holds what the choice left. Answers the updated list.
    ///
    /// # Errors
    /// `NotFound` (`noUndo`), `Conflict` (`changed` | `unsaved` | `linkInPath`), `Denied` (`denied`), `Trash`, IO.
    pub fn undo(&mut self, path: &str) -> Result<ChangeList, IpcError> {
        let done = self.step_back(path);
        self.answer(path, done)
    }

    fn step_back(&mut self, path: &str) -> Result<(), IpcError> {
        let at = self
            .undos
            .iter()
            .position(|u| u.path == path)
            .ok_or_else(|| refusal(ErrorKind::NotFound, path, "noUndo"))?;
        let undo = self.undos.remove(at);
        let done = match undo.place {
            Place::Real => self.undo_real(&undo),
            Place::Copy => self.undo_copy(&undo),
        };
        if done.is_err() {
            // still there for another try (the document saved, the file put back)
            self.undos.insert(at, undo);
            return done;
        }
        match undo.baseline {
            Some(base) => self.files.insert(undo.path, base),
            None => self.files.remove(&undo.path),
        };
        Ok(())
    }

    fn undo_real(&self, undo: &Undo) -> Result<(), IpcError> {
        let path = undo.path.as_str();
        if self.project.unsaved(path) {
            return Err(refusal(ErrorKind::Conflict, path, "unsaved"));
        }
        let abs = self.project.destination(path)?;
        let changed = || refusal(ErrorKind::Conflict, path, "changed");
        if Side::real(&abs)?.seen() != Some(undo.after) {
            return Err(changed());
        }
        match (&undo.before, undo.after) {
            (Held::Bytes(bytes), Seen::Bytes(written)) => {
                self.write_real(path, &abs, bytes, Some(written))?;
            }
            (Held::Bytes(bytes), Seen::Missing) => {
                self.write_real(path, &abs, bytes, None).map_err(|e| {
                    if e.kind == ErrorKind::Exists {
                        changed()
                    } else {
                        e
                    }
                })?;
            }
            (Held::Nothing, Seen::Bytes(_)) => self.recycle_real(path, &abs)?,
            (Held::Nothing, Seen::Missing) => {}
        }
        Ok(())
    }

    fn undo_copy(&self, undo: &Undo) -> Result<(), IpcError> {
        let path = undo.path.as_str();
        if self.read_copy(path)?.seen() != Some(undo.after) {
            return Err(refusal(ErrorKind::Conflict, path, "changed"));
        }
        match &undo.before {
            Held::Bytes(bytes) => self.put_copy(path, bytes),
            Held::Nothing => self.drop_copy(path),
        }
    }

    /// After a choice: a copy holding the real file's text with other line breaks | encoding takes the real bytes,
    /// so the file counts as unchanged from now on.
    fn settle(&mut self, path: &str) -> Result<(), IpcError> {
        let Some(base) = self.files.get(path).copied() else {
            return Ok(());
        };
        let real = Side::real(&self.project.resolve(path)?)?;
        let copy = self.read_copy(path)?;
        let (Side::Bytes(real_bytes), Side::Bytes(copy_bytes)) = (&real, &copy) else {
            return Ok(());
        };
        if fsx::hash(real_bytes) != base || real_bytes == copy_bytes {
            return Ok(());
        }
        let same = match (real.text(), copy.text()) {
            (Some(a), Some(b)) => lines::normalized(&a.text) == lines::normalized(&b.text),
            _ => false,
        };
        if same {
            self.put_copy(path, real_bytes)?;
        }
        Ok(())
    }

    /// Deletes the copy (its own folder only, never through a link; pending changes and undos go with it).
    ///
    /// # Errors
    /// IO.
    pub fn remove(self) -> Result<(), IpcError> {
        self.clear()
    }
}

#[cfg(test)]
mod tests;
