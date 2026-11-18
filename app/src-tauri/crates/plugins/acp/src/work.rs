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
use permark_core::paths::{fold, Root, Vault, VaultPath};
use permark_core::plugins::ProjectServices;
use permark_core::projects::Projects;
use permark_core::settings::ProjectConfig;
use permark_core::tree::Explorer;
use permark_core::watch::OwnWrites;
use permark_core::{ErrorKind, IpcError};
use permark_plugin_agent::guard::Guard;
use permark_plugin_review::{hunks, replace, Hunk};

use crate::dto::{FileChange, FileChangeKind, HunkView};

/// Largest file the copy takes (larger ones are skipped and counted) and the app splices.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Most files a copy holds.
pub const MAX_FILES: usize = 20_000;
/// Most bytes a copy holds.
pub const MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;
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
/// folders, right under the agent folder) stay out.
const AGENT_SETTINGS: [(&str, &[&str]); 8] = [
    (".claude", &["settings.json", "settings.local.json"]),
    (".kilocode", &["mcp.json"]),
    (".junie", &["mcp"]),
    (".kiro", &["settings", "hooks"]),
    (".clinerules", &["hooks"]),
    (".factory", &["settings.json", "mcp.json", "hooks"]),
    (".augment", &["settings.json"]),
    (".vibe", &["config.toml"]),
];

fn named(segment: &str, names: &[&str]) -> bool {
    let folded = fold(segment);
    names.iter().any(|name| folded == fold(name))
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
                    // Windows: a folder link goes with `remove_dir`
                    gone(std::fs::remove_file(&abs).or_else(|_| std::fs::remove_dir(&abs)))?;
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
}

/// The real file as a listing showed it, for a whole-file choice.
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
}

impl Side {
    fn read(abs: &Path) -> Result<Self, IpcError> {
        if abs.is_dir() {
            return Ok(Self::Missing);
        }
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
            Self::Missing | Self::Big => None,
        }
    }

    /// Text the app can splice back byte for byte (decoded without loss).
    fn text(&self) -> Option<Decoded> {
        match self {
            Self::Bytes(bytes) => fsx::decode(bytes).filter(|d| !d.lossy),
            Self::Missing | Self::Big => None,
        }
    }

    /// What a whole-file choice compares; `None` past the cap (cannot be compared).
    fn seen(&self) -> Option<Seen> {
        match self {
            Self::Missing => Some(Seen::Missing),
            Self::Bytes(bytes) => Some(Seen::Bytes(fsx::hash(bytes))),
            Self::Big => None,
        }
    }

    fn bytes(&self, path: &str) -> Result<&[u8], IpcError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::Big => Err(IpcError::new(ErrorKind::TooLarge).subject(path)),
            Self::Missing => Err(IpcError::new(ErrorKind::NotFound).subject(path)),
        }
    }
}

/// Lines of a normalized text (`None`: no file, no lines).
fn lines_of(text: Option<&str>) -> Vec<&str> {
    text.map_or_else(Vec::new, |t| t.split('\n').collect())
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
    fn view(&self, path: &str) -> FileChange {
        let old = lines_of(self.old.as_deref());
        let new = lines_of(self.new.as_deref());
        let owned = |lines: &[&str], range: &std::ops::Range<usize>| -> Vec<String> {
            lines
                .get(range.clone())
                .unwrap_or_default()
                .iter()
                .map(|l| (*l).to_owned())
                .collect()
        };
        FileChange {
            path: path.to_owned(),
            kind: self.kind,
            hunks: self
                .hunks
                .iter()
                .enumerate()
                .map(|(id, hunk)| HunkView {
                    id: u32::try_from(id).unwrap_or(u32::MAX),
                    old_start: u32::try_from(hunk.old.start + 1).unwrap_or(u32::MAX),
                    old_lines: owned(&old, &hunk.old),
                    new_lines: owned(&new, &hunk.new),
                })
                .collect(),
        }
    }
}

/// The hunk ids of a choice, sorted (`None`: every hunk).
fn pick(ids: Option<&[u32]>, count: usize, path: &str) -> Result<Vec<usize>, IpcError> {
    let Some(ids) = ids else {
        return Ok((0..count).collect());
    };
    let picked: BTreeSet<usize> = ids
        .iter()
        .filter_map(|id| usize::try_from(*id).ok())
        .collect();
    if picked.is_empty() || picked.len() != ids.len() || picked.iter().any(|id| *id >= count) {
        return Err(refusal(ErrorKind::InvalidData, path, "unknownHunk"));
    }
    Ok(picked.into_iter().collect())
}

/// A file only chosen as a whole: hunk ids allowed only when they name every hunk.
fn whole_only(ids: Option<&[u32]>, count: usize, path: &str) -> Result<(), IpcError> {
    match ids {
        None => Ok(()),
        Some(ids) if count > 0 && pick(Some(ids), count, path)?.len() == count => Ok(()),
        Some(_) => Err(refusal(ErrorKind::InvalidData, path, "wholeFileOnly")),
    }
}

fn write_copy(abs: &Path, bytes: &[u8], path: &str) -> Result<(), IpcError> {
    fsx::write_atomic(abs, bytes).map_err(|e| IpcError::io(path, &e))
}

fn remove_copy(abs: &Path, path: &str) -> Result<(), IpcError> {
    match std::fs::remove_file(abs) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(IpcError::io(path, &e)),
        _ => Ok(()),
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

    /// The copy of a project path: always inside the copy (a path climbing out is refused).
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

    /// Brings the person's changes into the copy (before each prompt): a real file changed where the copy is
    /// unchanged replaces the copy; new real files join it; a real file gone (or now hidden) leaves an unchanged copy.
    /// Where both changed, the agent's version stays (listed as a conflict).
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
            let copy_abs = self.copy_path(&path)?;
            match self.files.get(&path).copied() {
                Some(base) if base == real => {}
                Some(base) => {
                    if Side::read(&copy_abs)?.hash() == Some(base) {
                        write_copy(&copy_abs, &bytes, &path)?;
                        self.files.insert(path, real);
                    }
                }
                // a file the agent made at that path stays (listed as added)
                None => {
                    if matches!(Side::read(&copy_abs)?, Side::Missing) {
                        write_copy(&copy_abs, &bytes, &path)?;
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
            let copy_abs = self.copy_path(&path)?;
            match Side::read(&copy_abs)? {
                Side::Bytes(bytes) if fsx::hash(&bytes) == base => {
                    remove_copy(&copy_abs, &path)?;
                    self.files.remove(&path);
                }
                Side::Missing => {
                    self.files.remove(&path);
                }
                // changed in the copy: listed as a conflict
                Side::Bytes(_) | Side::Big => {}
            }
        }
        Ok(())
    }

    /// Files of the copy (the ignore files it holds apply; agent settings and machinery left out).
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
            if !entry.file_type().is_some_and(|t| t.is_file()) {
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
        let real = Side::read(&self.project.resolve(path)?)?;
        let copy = Side::read(&self.copy_path(path)?)?;
        let baseline = self.files.get(path).copied();
        let kind = match (baseline, &copy) {
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
            FileChangeKind::Binary | FileChangeKind::Conflict => (kind, None, None),
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

    /// Every file whose copy differs from the real file, by path.
    ///
    /// # Errors
    /// A file of either side cannot be read; the copy holds far too many files.
    pub fn changes(&mut self) -> Result<Vec<FileChange>, IpcError> {
        let mut paths: BTreeSet<String> = self.files.keys().cloned().collect();
        paths.extend(self.copy_files()?);
        self.reviewed.clear();
        let mut out = Vec::new();
        for path in paths {
            let Some(look) = self.look(&path)? else {
                continue;
            };
            if matches!(look.kind, FileChangeKind::Binary | FileChangeKind::Conflict) {
                if let Some(state) = look.real.seen() {
                    self.reviewed.insert(path.clone(), state);
                }
            }
            out.push(look.view(&path));
        }
        Ok(out)
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

    /// Applies the agent's change to the real file: the chosen hunks (`None`: all), or the whole file for the kinds
    /// that have no hunks. Refused while a window holds unsaved edits of the file, and when the real file changed
    /// since the copy was taken (whole file: since it was listed). Answers the fresh list.
    ///
    /// # Errors
    /// `NotFound` (`noChange`), `Conflict` (`unsaved` | `changed`), `Exists` (a new file where one appeared),
    /// `InvalidData` (`unknownHunk` | `wholeFileOnly`), `TooLarge`, `Trash`, IO.
    pub fn accept(
        &mut self,
        path: &str,
        hunk_ids: Option<&[u32]>,
    ) -> Result<Vec<FileChange>, IpcError> {
        let look = self
            .look(path)?
            .ok_or_else(|| refusal(ErrorKind::NotFound, path, "noChange"))?;
        if self.project.unsaved(path) {
            return Err(refusal(ErrorKind::Conflict, path, "unsaved"));
        }
        let abs = self.project.resolve(path)?;
        let baseline = self.files.get(path).copied();
        match look.kind {
            FileChangeKind::Modified => {
                let picked = pick(hunk_ids, look.hunks.len(), path)?;
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
                let hash =
                    self.write_real(path, &abs, &lines::encode(&text, real.encoding), baseline)?;
                self.files.insert(path.to_owned(), hash);
            }
            FileChangeKind::Added => {
                whole_only(hunk_ids, look.hunks.len(), path)?;
                let hash = self.write_real(path, &abs, look.copy.bytes(path)?, None)?;
                self.files.insert(path.to_owned(), hash);
            }
            FileChangeKind::Deleted => {
                whole_only(hunk_ids, look.hunks.len(), path)?;
                self.recycle_real(path, &abs)?;
                self.files.remove(path);
            }
            FileChangeKind::Binary => {
                whole_only(hunk_ids, 0, path)?;
                let hash = self.write_real(path, &abs, look.copy.bytes(path)?, baseline)?;
                self.files.insert(path.to_owned(), hash);
            }
            FileChangeKind::Conflict => {
                if hunk_ids.is_some() {
                    return Err(refusal(ErrorKind::Conflict, path, "changed"));
                }
                let now = look
                    .real
                    .seen()
                    .ok_or_else(|| IpcError::new(ErrorKind::TooLarge).subject(path))?;
                if self.reviewed.get(path) != Some(&now) {
                    return Err(refusal(ErrorKind::Conflict, path, "changed"));
                }
                match &look.copy {
                    Side::Missing => {
                        self.recycle_real(path, &abs)?;
                        self.files.remove(path);
                    }
                    copy => {
                        let hash = self.write_real(path, &abs, copy.bytes(path)?, now.hash())?;
                        self.files.insert(path.to_owned(), hash);
                    }
                }
            }
        }
        self.settle(path)?;
        self.changes()
    }

    /// Drops the agent's change in the copy: the chosen hunks of a modified file get the real lines back; anything
    /// else (`None`: the whole file) takes the real file as it is now. Answers the fresh list.
    ///
    /// # Errors
    /// `NotFound` (`noChange`), `InvalidData` (`unknownHunk` | `wholeFileOnly`), IO.
    pub fn reject(
        &mut self,
        path: &str,
        hunk_ids: Option<&[u32]>,
    ) -> Result<Vec<FileChange>, IpcError> {
        let look = self
            .look(path)?
            .ok_or_else(|| refusal(ErrorKind::NotFound, path, "noChange"))?;
        let copy_abs = self.copy_path(path)?;
        match look.kind {
            FileChangeKind::Modified if hunk_ids.is_some() => {
                let picked = pick(hunk_ids, look.hunks.len(), path)?;
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
                write_copy(&copy_abs, &lines::encode(&text, copy.encoding), path)?;
            }
            FileChangeKind::Added => {
                whole_only(hunk_ids, look.hunks.len(), path)?;
                remove_copy(&copy_abs, path)?;
            }
            kind => {
                let count = if kind == FileChangeKind::Deleted {
                    look.hunks.len()
                } else {
                    0
                };
                whole_only(hunk_ids, count, path)?;
                match &look.real {
                    Side::Bytes(bytes) => {
                        write_copy(&copy_abs, bytes, path)?;
                        self.files.insert(path.to_owned(), fsx::hash(bytes));
                    }
                    // gone | past the cap: out of the copy
                    Side::Missing | Side::Big => {
                        remove_copy(&copy_abs, path)?;
                        self.files.remove(path);
                    }
                }
            }
        }
        self.settle(path)?;
        self.changes()
    }

    /// After a choice: a copy holding the real file's text with other line breaks | encoding takes the real bytes,
    /// so the file counts as unchanged from now on.
    fn settle(&mut self, path: &str) -> Result<(), IpcError> {
        let Some(base) = self.files.get(path).copied() else {
            return Ok(());
        };
        let copy_abs = self.copy_path(path)?;
        let real = Side::read(&self.project.resolve(path)?)?;
        let copy = Side::read(&copy_abs)?;
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
            write_copy(&copy_abs, real_bytes, path)?;
        }
        Ok(())
    }

    /// Deletes the copy (its own folder only; pending changes go with it).
    ///
    /// # Errors
    /// IO.
    pub fn remove(self) -> Result<(), IpcError> {
        self.clear()
    }
}

#[cfg(test)]
mod tests;
