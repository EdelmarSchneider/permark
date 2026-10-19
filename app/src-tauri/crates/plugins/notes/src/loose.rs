//! Loose files: files the person opened in the Notes window that are not in the notes folder (a Markdown file someone
//! sent, one in Downloads, a text file). They stay in the list under **Files** until they are removed from it, and
//! removing one never touches the file.
//!
//! A loose file is a scope of its own: the window refers to it by an opaque id, never by a path of the disk. Its
//! document path is `@file/<id>/<file name>` (core's `NotesScope`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use permark_core::error::{ErrorKind, IpcError};
use permark_core::fsx;
use permark_core::notes::{loose_key, loose_path, parse_loose, LOOSE_PREFIX};
use permark_core::paths::{fold, FileScope};

use crate::journal::Entry as JournalEntry;
use crate::names::{edits_stem, first_free, EXTENSION};
use crate::service::{
    checked_name, differs_from_file, write_like, write_new, Ctx, NotesService, Target,
};
use crate::state::{LooseEntry, Stamp};
use crate::text::{is_blank, stem_from_title, title_of};
use crate::types::{EntryKind, LooseState, NoteEntry};

/// How many loose files are kept, besides the pinned ones and the ones with unsaved text.
pub const MAX_LISTED: usize = 50;

/// A new id: eight hexadecimal digits made from `seed` (the path and the time), different from every id `taken` knows.
#[must_use]
pub fn new_id(seed: &str, now_ms: u64, taken: impl Fn(&str) -> bool) -> String {
    let mut bytes = seed.as_bytes().to_vec();
    bytes.extend_from_slice(&now_ms.to_le_bytes());
    for round in 0_u32.. {
        let mut salted = bytes.clone();
        salted.extend_from_slice(&round.to_le_bytes());
        let id = format!("{:08x}", fsx::hash(&salted).0 & 0xffff_ffff);
        if !taken(&id) {
            return id;
        }
    }
    // unreachable in practice: a free id is found within a few rounds
    format!("{:08x}", now_ms & 0xffff_ffff)
}

/// The folder of a file as the list shows it: with `~` for the home folder.
#[must_use]
pub fn folder_label(file: &Path, home: Option<&Path>) -> String {
    let Some(folder) = file.parent() else {
        return String::new();
    };
    if let Some(home) = home {
        if let Ok(rest) = folder.strip_prefix(home) {
            let sep = std::path::MAIN_SEPARATOR;
            return if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~{sep}{}", rest.display())
            };
        }
    }
    folder.display().to_string()
}

/// The file that `entry` became when it was renamed in its folder: the one file there with the size and modification
/// time the entry had, that `taken` does not claim (another entry's). `None` when there is no such file or more than one.
#[must_use]
pub fn find_renamed(entry: &LooseEntry, taken: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    if entry.stamp == Stamp::default() {
        return None;
    }
    let folder = Path::new(&entry.path).parent()?;
    let mut found: Option<PathBuf> = None;
    for item in std::fs::read_dir(folder).ok()?.filter_map(Result::ok) {
        let path = item.path();
        if taken(&path) || Stamp::of(&path) != Some(entry.stamp) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(path);
    }
    found
}

/// Keeps the list to [`MAX_LISTED`] files: the oldest are dropped, except the ones `keep` says must stay.
pub fn prune(files: &mut Vec<LooseEntry>, keep: impl Fn(&LooseEntry) -> bool) {
    let mut candidates: Vec<(u64, String)> = files
        .iter()
        .filter(|file| !keep(file))
        .map(|file| (file.opened_ms, file.id.clone()))
        .collect();
    if candidates.len() <= MAX_LISTED {
        return;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let dropped: Vec<String> = candidates
        .into_iter()
        .skip(MAX_LISTED)
        .map(|(_, id)| id)
        .collect();
    files.retain(|file| !dropped.contains(&file.id));
}

impl NotesService {
    /// The loose files as list rows (their state worked out now).
    pub(crate) fn loose_entries(
        &self,
        ctx: &Ctx,
        journal: &HashMap<String, JournalEntry>,
    ) -> Vec<NoteEntry> {
        let (mut entries, pins) = ctx.read(|s| (s.loose.clone(), s.pins.clone()));
        // a file renamed in its folder is found again (the one with the stamp it had)
        let mut moved = Vec::new();
        for entry in &entries {
            if Stamp::of(Path::new(&entry.path)).is_none() {
                let claimed: Vec<String> = entries.iter().map(|e| fold(&e.path)).collect();
                let found = find_renamed(entry, |p| claimed.contains(&fold(&p.to_string_lossy())));
                if let Some(found) = found {
                    moved.push((entry.id.clone(), found));
                }
            }
        }
        for (id, found) in &moved {
            if self.scope.set_loose(id, found).is_ok() {
                let path = self.scope.loose_file(id).unwrap_or_else(|| found.clone());
                ctx.update(|state| {
                    if let Some(entry) = state.loose.iter_mut().find(|e| e.id == *id) {
                        entry.path = path.display().to_string();
                        entry.stamp = Stamp::of(&path).unwrap_or(entry.stamp);
                    }
                });
            }
        }
        if !moved.is_empty() {
            entries = ctx.read(|s| s.loose.clone());
        }
        let home = dirs::home_dir();
        let mut rows: Vec<(u64, NoteEntry)> = entries
            .iter()
            .map(|entry| {
                let abs = Path::new(&entry.path);
                let name = abs
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let now = Stamp::of(abs);
                let kept = journal.get(&loose_key(&entry.id));
                let pending = kept.is_some_and(|k| match now {
                    Some(_) => differs_from_file(k, abs),
                    None => !is_blank(&k.text),
                });
                let state = match now {
                    None => LooseState::Gone,
                    Some(_) if pending => LooseState::Unsaved,
                    Some(stamp) if entry.stamp != Stamp::default() && stamp != entry.stamp => {
                        LooseState::ChangedOnDisk
                    }
                    Some(_) => LooseState::Clean,
                };
                let key = loose_key(&entry.id);
                (
                    entry.opened_ms,
                    NoteEntry {
                        path: loose_path(&entry.id, &name),
                        kind: EntryKind::File,
                        title: name.clone(),
                        snippet: String::new(),
                        modified_ms: now.map_or(0, |s| s.modified_ms),
                        size: now.map_or(0, |s| s.size),
                        pinned: pins.contains(&key),
                        shared: false,
                        copied: false,
                        recovered: false,
                        folder: Some(folder_label(abs, home.as_deref())),
                        state: Some(state),
                        pending,
                        name,
                    },
                )
            })
            .collect();
        rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// The loose file was opened: it is the one to come back to and goes to the top of the files; what it looks like now
    /// is what "changed on disk" is measured against.
    pub(crate) fn loose_opened(ctx: &Ctx, id: &str, now_ms: u64) {
        ctx.update(|state| {
            if let Some(entry) = state.loose.iter_mut().find(|e| e.id == id) {
                entry.opened_ms = now_ms;
                if let Some(stamp) = Stamp::of(Path::new(&entry.path)) {
                    entry.stamp = stamp;
                }
                state.open = Some(loose_key(id));
            }
        });
    }

    /// The editor saved the loose file: its stamp is what the file looks like now.
    pub(crate) fn loose_saved(&self, path: &str) {
        let ctx = self.ctx();
        let Some((id, _)) = parse_loose(path) else {
            return;
        };
        ctx.update(|state| {
            if let Some(entry) = state.loose.iter_mut().find(|e| e.id == id) {
                if let Some(stamp) = Stamp::of(Path::new(&entry.path)) {
                    entry.stamp = stamp;
                }
            }
        });
    }

    /// Lists a file in the Files group (a file that is a note of the folder is that note). Returns its document path.
    ///
    /// # Errors
    /// The file is not there | is not a file.
    pub fn open_file(&self, path: &str, now_ms: u64) -> Result<String, IpcError> {
        let ctx = self.ctx();
        let scope = FileScope::open(Path::new(path)).map_err(IpcError::from)?;
        let file = scope.file().to_path_buf();
        // a file of the notes folder is a note, not a loose file
        if let Some(parent) = file.parent() {
            if crate::folder::same_place(parent, ctx.folder()) {
                if let Some(name) = file.file_name().map(|n| n.to_string_lossy().into_owned()) {
                    if crate::names::is_note_name(&name) {
                        return Ok(name);
                    }
                }
            }
        }
        let text = file.display().to_string();
        let existing = ctx.read(|s| {
            s.loose
                .iter()
                .find(|e| fold(&e.path) == fold(&text))
                .map(|e| e.id.clone())
        });
        let id = existing.unwrap_or_else(|| Self::add_loose(&ctx, &file, &text, now_ms));
        self.scope.set_loose(&id, &file).map_err(IpcError::from)?;
        Self::loose_opened(&ctx, &id, now_ms);
        Ok(loose_path(&id, &scope.name()))
    }

    /// A new entry for the file; the list stays within its limit.
    fn add_loose(ctx: &Ctx, file: &Path, text: &str, now_ms: u64) -> String {
        let taken: Vec<String> = ctx.read(|s| s.loose.iter().map(|e| e.id.clone()).collect());
        let id = new_id(text, now_ms, |candidate| {
            taken.iter().any(|t| t == candidate)
        });
        let stamp = Stamp::of(file).unwrap_or_default();
        // files with text typed and not saved stay whatever the limit says
        let pending: Vec<String> = ctx
            .journal
            .all()
            .iter()
            .filter_map(|e| permark_core::notes::loose_id_of(&e.key).map(str::to_owned))
            .collect();
        ctx.update(|state| {
            state.loose.push(LooseEntry {
                id: id.clone(),
                path: text.to_owned(),
                opened_ms: now_ms,
                stamp,
            });
            let pins = state.pins.clone();
            prune(&mut state.loose, |e| {
                pins.contains(&loose_key(&e.id)) || pending.contains(&e.id)
            });
        });
        id
    }

    fn loose_entry(ctx: &Ctx, path: &str) -> Result<LooseEntry, IpcError> {
        let Target::Loose { id } = Self::target(path)? else {
            return Err(IpcError::new(ErrorKind::Unsupported).subject(path));
        };
        ctx.read(|s| s.loose.iter().find(|e| e.id == id).cloned())
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(path))
    }

    /// Takes a file off the list. The file itself is never touched; text typed into it and not saved goes with it.
    ///
    /// # Errors
    /// Not listed.
    pub fn remove_file(&self, path: &str) -> Result<(), IpcError> {
        let ctx = self.ctx();
        let entry = Self::loose_entry(&ctx, path)?;
        ctx.journal.clear(&loose_key(&entry.id));
        self.scope.remove_loose(&entry.id);
        let key = loose_key(&entry.id);
        ctx.update(|state| {
            state.loose.retain(|e| e.id != entry.id);
            state.pins.retain(|p| *p != key);
            if state.open.as_deref() == Some(key.as_str()) {
                state.open = None;
            }
        });
        Ok(())
    }

    /// A file that was not where it was has been found by the person: the entry points there.
    ///
    /// # Errors
    /// Not listed | the new place is not a file.
    pub fn locate_file(&self, path: &str, new_path: &str, now_ms: u64) -> Result<String, IpcError> {
        let ctx = self.ctx();
        let entry = Self::loose_entry(&ctx, path)?;
        let scope = FileScope::open(Path::new(new_path)).map_err(IpcError::from)?;
        self.scope
            .set_loose(&entry.id, scope.file())
            .map_err(IpcError::from)?;
        let text = scope.file().display().to_string();
        ctx.update(|state| {
            if let Some(found) = state.loose.iter_mut().find(|e| e.id == entry.id) {
                found.path.clone_from(&text);
                found.stamp = Stamp::of(scope.file()).unwrap_or_default();
                found.opened_ms = now_ms;
            }
        });
        Ok(loose_path(&entry.id, &scope.name()))
    }

    /// The person takes the file's own version: what was typed is let go.
    ///
    /// # Errors
    /// Not listed.
    pub fn reload_file(&self, path: &str) -> Result<(), IpcError> {
        let ctx = self.ctx();
        let entry = Self::loose_entry(&ctx, path)?;
        ctx.journal.clear(&loose_key(&entry.id));
        Self::loose_opened(&ctx, &entry.id, entry.opened_ms);
        Ok(())
    }

    /// The person keeps what was typed: it is written over the file (or the file is made again when it is gone).
    ///
    /// # Errors
    /// Not listed | nothing typed is kept | the system refused.
    pub fn keep_mine(&self, path: &str) -> Result<(), IpcError> {
        let ctx = self.ctx();
        let entry = Self::loose_entry(&ctx, path)?;
        let key = loose_key(&entry.id);
        let kept = ctx
            .journal
            .load(&key)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("pending"))?;
        let abs = self
            .scope
            .loose_file(&entry.id)
            .unwrap_or_else(|| PathBuf::from(&entry.path));
        write_like(&abs, &kept.text).map_err(|e| IpcError::io(path, &e))?;
        ctx.journal.clear(&key);
        let _ = self.scope.set_loose(&entry.id, &abs);
        self.loose_saved(&loose_path(&entry.id, "x"));
        Ok(())
    }

    /// What was typed into a loose file becomes a note of its own ("<name> (my edits)"). Returns the note's name.
    ///
    /// # Errors
    /// Not listed | nothing typed is kept | the folder is missing | read-only.
    pub fn save_as_note(&self, path: &str) -> Result<String, IpcError> {
        let ctx = self.ctx();
        let entry = Self::loose_entry(&ctx, path)?;
        let kept = ctx
            .journal
            .load(&loose_key(&entry.id))
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject("pending"))?;
        let dir = self.ready_folder(&ctx)?;
        let base = Path::new(&entry.path)
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let stem = stem_from_title(&title_of(&kept.text)).unwrap_or(base);
        let name = first_free(&edits_stem(&stem), " ", |n| dir.join(n).exists());
        write_new(&dir.join(&name), kept.text.as_bytes()).map_err(|e| IpcError::io(&name, &e))?;
        ctx.journal.clear(&loose_key(&entry.id));
        Ok(name)
    }

    /// Renames the real file next to itself and keeps the entry (the id, so pins and pending text follow).
    pub(crate) fn rename_loose(
        &self,
        ctx: &Ctx,
        id: &str,
        typed: &str,
    ) -> Result<String, IpcError> {
        let entry = ctx
            .read(|s| s.loose.iter().find(|e| e.id == id).cloned())
            .ok_or_else(|| {
                IpcError::new(ErrorKind::NotFound).subject(format!("{LOOSE_PREFIX}{id}"))
            })?;
        let old = Path::new(&entry.path);
        let extension = old.extension().map_or_else(
            || EXTENSION.to_owned(),
            |e| e.to_string_lossy().into_owned(),
        );
        let name = checked_name(typed, Some(&extension))?;
        let folder = old
            .parent()
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(&entry.path))?;
        if !old.is_file() {
            return Err(IpcError::new(ErrorKind::NotFound).subject(&entry.path));
        }
        let target = folder.join(&name);
        let current = old
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if name != current {
            if target.exists() && fold(&name) != fold(&current) {
                return Err(IpcError::new(ErrorKind::Exists).subject(name));
            }
            std::fs::rename(old, &target).map_err(|e| IpcError::io(&name, &e))?;
        }
        self.scope.set_loose(id, &target).map_err(IpcError::from)?;
        let shown = self.scope.loose_file(id).unwrap_or(target);
        ctx.update(|state| {
            if let Some(found) = state.loose.iter_mut().find(|e| e.id == id) {
                found.path = shown.display().to_string();
                found.stamp = Stamp::of(&shown).unwrap_or(found.stamp);
            }
        });
        Ok(loose_path(id, &name))
    }
}
