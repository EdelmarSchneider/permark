//! The copy engine of "drop into project": files and folders copied chunk by chunk under a hidden temp name,
//! progress and cancel checked between chunks, links never followed, originals deleted only while they still are
//! what was copied.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use super::{ImportPhase, ImportProgress};
use crate::fsx::{retry, scratch_in, RETRY_DELAYS};

const CHUNK: usize = 1024 * 1024;
/// Progress is reported at most this often (and at the end of each item).
pub(crate) const TICK: Duration = Duration::from_millis(100);

/// Why a copy stopped.
#[derive(Debug)]
pub(crate) enum Stop {
    Cancelled,
    Failed { path: PathBuf, source: io::Error },
}

impl Stop {
    pub(crate) fn failed(path: &Path, source: io::Error) -> Self {
        Self::Failed {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// The state of one import: what has been done, what is left, whether the person cancelled, and where progress goes.
pub(crate) struct Run<'a> {
    plan_id: u64,
    cancel: &'a AtomicBool,
    report: &'a dyn Fn(&ImportProgress),
    pub phase: ImportPhase,
    pub item: String,
    pub items_done: u32,
    pub items_total: u32,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Links and special files left out of folders (never followed, never copied).
    pub links: u32,
    tick: Duration,
    last: Option<Instant>,
    buf: Vec<u8>,
}

impl<'a> Run<'a> {
    pub(crate) fn new(
        plan_id: u64,
        items_total: u32,
        cancel: &'a AtomicBool,
        report: &'a dyn Fn(&ImportProgress),
        tick: Duration,
    ) -> Self {
        Self {
            plan_id,
            cancel,
            report,
            phase: ImportPhase::Counting,
            item: String::new(),
            items_done: 0,
            items_total,
            files_done: 0,
            files_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            links: 0,
            tick,
            last: None,
            buf: Vec::new(),
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn check(&self) -> Result<(), Stop> {
        if self.cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Reports progress: at most once per tick, unless `force`.
    pub(crate) fn emit(&mut self, force: bool) {
        let now = Instant::now();
        if !force
            && self
                .last
                .is_some_and(|at| now.duration_since(at) < self.tick)
        {
            return;
        }
        self.last = Some(now);
        (self.report)(&ImportProgress {
            plan_id: self.plan_id,
            phase: self.phase,
            items_done: self.items_done,
            items_total: self.items_total,
            files_done: self.files_done,
            files_total: self.files_total.max(self.files_done),
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total.max(self.bytes_done),
            item: self.item.clone(),
        });
    }
}

/// A link (symlink, junction) is never followed: on Windows a reparse point that is a folder counts as one too, a file
/// with a reparse point (a cloud placeholder) is an ordinary file.
pub(crate) fn is_link(file_type: fs::FileType, meta: Option<&fs::Metadata>) -> bool {
    if file_type.is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        const DIRECTORY: u32 = 0x10;
        meta.is_some_and(|m| {
            let attributes = m.file_attributes();
            attributes & REPARSE_POINT != 0 && attributes & DIRECTORY != 0
        })
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

/// One file seen while copying: enough to tell later whether the original still is what was copied.
#[derive(Debug, Clone)]
pub(crate) struct CopiedFile {
    pub rel: PathBuf,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

/// What a copy put in place: the files (with what they were) and the folders made, relative to the item.
#[derive(Debug, Default)]
pub(crate) struct Copied {
    pub files: Vec<CopiedFile>,
    pub dirs: Vec<PathBuf>,
}

/// Copies one regular file to `dst`, which must not exist. Modified time and the read-only flag are kept.
fn copy_file(run: &mut Run<'_>, src: &Path, dst: &Path) -> Result<(u64, Option<SystemTime>), Stop> {
    run.check()?;
    let mut input =
        retry(&RETRY_DELAYS, || fs::File::open(src)).map_err(|e| Stop::failed(src, e))?;
    let meta = input.metadata().map_err(|e| Stop::failed(src, e))?;
    if !meta.is_file() {
        return Err(Stop::failed(
            src,
            io::Error::other("not an ordinary file any more"),
        ));
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
        .map_err(|e| Stop::failed(dst, e))?;
    let mut buf = std::mem::take(&mut run.buf);
    if buf.len() != CHUNK {
        buf = vec![0; CHUNK];
    }
    let result = loop {
        if run.cancelled() {
            break Err(Stop::Cancelled);
        }
        let read = match input.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => break Err(Stop::failed(src, e)),
        };
        if let Err(e) = output.write_all(&buf[..read]) {
            break Err(Stop::failed(dst, e));
        }
        run.bytes_done += read as u64;
        run.emit(false);
    };
    run.buf = buf;
    result?;
    let modified = meta.modified().ok();
    if let Some(time) = modified {
        // best effort: a file system that cannot keep times still has the bytes
        let _ = output.set_modified(time);
    }
    drop(output);
    let _ = fs::set_permissions(dst, meta.permissions());
    run.files_done += 1;
    Ok((meta.len(), modified))
}

/// Copies the folder `src` into the new folder `dst` (made here). Explicit stack: no depth limit.
fn copy_tree(run: &mut Run<'_>, src: &Path, dst: &Path, copied: &mut Copied) -> Result<(), Stop> {
    fs::create_dir(dst).map_err(|e| Stop::failed(dst, e))?;
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf(), PathBuf::new())];
    while let Some((from, to, rel)) = stack.pop() {
        run.check()?;
        let entries =
            retry(&RETRY_DELAYS, || fs::read_dir(&from)).map_err(|e| Stop::failed(&from, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Stop::failed(&from, e))?;
            run.check()?;
            let name = entry.file_name();
            let path = entry.path();
            let file_type = entry.file_type().map_err(|e| Stop::failed(&path, e))?;
            let meta = entry.metadata().ok();
            if is_link(file_type, meta.as_ref()) || !(file_type.is_dir() || file_type.is_file()) {
                run.links += 1;
                continue;
            }
            let target = to.join(&name);
            let below = rel.join(&name);
            if file_type.is_dir() {
                fs::create_dir(&target).map_err(|e| Stop::failed(&target, e))?;
                copied.dirs.push(below.clone());
                stack.push((path, target, below));
            } else {
                let (len, modified) = copy_file(run, &path, &target)?;
                copied.files.push(CopiedFile {
                    rel: below,
                    len,
                    modified,
                });
            }
        }
    }
    Ok(())
}

/// Totals the files and bytes below `src` into `run` (links not followed).
pub(crate) fn count_tree(run: &mut Run<'_>, src: &Path) -> Result<(), Stop> {
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        run.check()?;
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let meta = entry.metadata().ok();
            if is_link(file_type, meta.as_ref()) {
                continue;
            }
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                run.files_total += 1;
                run.bytes_total += meta.map_or(0, |m| m.len());
            }
        }
        run.emit(false);
    }
    Ok(())
}

/// Makes the item at `dest` (which must be free) as a copy of `src`: built under a hidden temp name beside `dest`,
/// renamed into place only when whole. `announce` hears the files (relative) right before the rename. A cancel or a
/// failure removes the temp.
pub(crate) fn place_copy(
    run: &mut Run<'_>,
    src: &Path,
    is_dir: bool,
    dest: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
    announce: &mut dyn FnMut(&Copied),
) -> Result<Copied, Stop> {
    let stage = scratch_in(dest.parent().unwrap_or_else(|| Path::new(".")));
    let mut copied = Copied::default();
    let built = if is_dir {
        copy_tree(run, src, &stage, &mut copied)
    } else {
        copy_file(run, src, &stage).map(|(len, modified)| {
            copied.files.push(CopiedFile {
                rel: PathBuf::new(),
                len,
                modified,
            });
        })
    };
    if let Err(stop) = built {
        remove_tree_forced(&stage);
        return Err(stop);
    }
    if run.cancelled() {
        remove_tree_forced(&stage);
        return Err(Stop::Cancelled);
    }
    if fs::symlink_metadata(dest).is_ok() {
        remove_tree_forced(&stage);
        return Err(Stop::failed(
            dest,
            io::Error::from(io::ErrorKind::AlreadyExists),
        ));
    }
    announce(&copied);
    if let Err(error) = retry(&RETRY_DELAYS, || rename(&stage, dest)) {
        remove_tree_forced(&stage);
        return Err(Stop::failed(dest, error));
    }
    Ok(copied)
}

/// Whether an error from a rename means "not on this volume".
pub(crate) fn crosses_volumes(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    #[cfg(windows)]
    {
        // ERROR_NOT_SAME_DEVICE
        error.raw_os_error() == Some(17)
    }
    #[cfg(not(windows))]
    {
        // EXDEV
        error.raw_os_error() == Some(18)
    }
}

/// Removes a file whatever its read-only flag says; transient locks retried.
fn remove_file_forced(path: &Path) -> io::Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.permissions().readonly() {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            // a file the person moved away, deleted next
            permissions.set_readonly(false);
            let _ = fs::set_permissions(path, permissions);
        }
    }
    retry(&RETRY_DELAYS, || fs::remove_file(path))
}

/// Removes a temp file or folder, read-only files included. Best effort: a temp left behind is hidden scratch.
pub(crate) fn remove_tree_forced(path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if !meta.is_dir() {
        let _ = remove_file_forced(path);
        return;
    }
    if fs::remove_dir_all(path).is_ok() {
        return;
    }
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(entry.path()),
                Ok(_) => {
                    let _ = remove_file_forced(&entry.path());
                }
                Err(_) => {}
            }
        }
    }
    let _ = fs::remove_dir_all(path);
}

/// What deleting the originals of a copied item left in place.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Deleted {
    /// Originals removed.
    pub removed: u32,
    /// Originals still there: changed since the copy, locked, or not deleted because the person cancelled.
    pub left: u32,
}

/// After a copy across volumes: deletes the originals that still are what was copied (same length and modified time),
/// then the emptied folders, deepest first. A file changed since is kept.
pub(crate) fn delete_originals(
    run: &Run<'_>,
    src: &Path,
    is_dir: bool,
    copied: &Copied,
) -> Deleted {
    let mut out = Deleted::default();
    for file in &copied.files {
        let path = if is_dir {
            src.join(&file.rel)
        } else {
            src.to_path_buf()
        };
        let unchanged = fs::symlink_metadata(&path).is_ok_and(|m| {
            m.is_file() && m.len() == file.len && m.modified().ok() == file.modified
        });
        if !unchanged || run.cancelled() || remove_file_forced(&path).is_err() {
            out.left += 1;
        } else {
            out.removed += 1;
        }
    }
    if is_dir {
        let mut dirs: Vec<&PathBuf> = copied.dirs.iter().collect();
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        for dir in dirs {
            // not empty (a file kept, a link): stays, and so do its parents
            let _ = fs::remove_dir(src.join(dir));
        }
        let _ = fs::remove_dir(src);
    }
    out
}

/// Moves `src` to `dest` (which must be free): one rename, or, across volumes, a copy and then the deletion of the
/// original. `Ok(false)`: some of the original stayed.
pub(crate) fn relocate(
    src: &Path,
    is_dir: bool,
    dest: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<bool, Stop> {
    match retry(&RETRY_DELAYS, || rename(src, dest)) {
        Ok(()) => Ok(true),
        Err(error) if crosses_volumes(&error) => {
            let cancel = AtomicBool::new(false);
            let silent = |_: &ImportProgress| {};
            let mut run = Run::new(0, 1, &cancel, &silent, TICK);
            let copied = place_copy(&mut run, src, is_dir, dest, rename, &mut |_| {})?;
            let deleted = delete_originals(&run, src, is_dir, &copied);
            Ok(deleted.left == 0 && run.links == 0)
        }
        Err(error) => Err(Stop::failed(src, error)),
    }
}
