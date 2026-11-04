//! One file into the store: `<name>.part` + `<name>.part.meta` next to the target. Meta = pins + unsigned address
//! (NEVER the resolved signed one). sha256 while streaming; resume by `Range` after rehashing the part; never read
//! past the pinned size; final atomic move after the pin matches; mismatch -> part + meta removed.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{open, Asset, DownloadError, Phase, Progress, Response, Secret, Transport};
use crate::fsx;

/// Read | hash buffer.
const CHUNK: usize = 64 * 1024;
/// Meta size read back.
const META_LIMIT: u64 = 64 * 1024;
/// Least time between two progress updates (as token-count's vocab downloads).
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Target, part, meta of one asset in a folder.
pub(super) struct Files {
    pub(super) target: PathBuf,
    pub(super) part: PathBuf,
    pub(super) meta: PathBuf,
}

impl Files {
    pub(super) fn of(dir: &Path, asset: &Asset) -> Self {
        Self::named(dir, asset.name())
    }

    fn named(dir: &Path, name: &str) -> Self {
        Self {
            target: dir.join(name),
            part: dir.join(format!("{name}.part")),
            meta: dir.join(format!("{name}.part.meta")),
        }
    }
}

/// What a part was started for: resumed ONLY when the asset's pins still say the same.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PartMeta {
    url: String,
    bytes: u64,
    sha256: String,
    revision: Option<String>,
}

impl PartMeta {
    pub(super) fn of(asset: &Asset) -> Self {
        Self {
            url: asset.url(),
            bytes: asset.bytes(),
            sha256: sha_hex(asset.sha256()),
            revision: asset.revision().map(str::to_owned),
        }
    }

    fn read(path: &Path) -> Option<Self> {
        let bytes = fsx::read_bytes(path, META_LIMIT).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// Lowercase hex of a sha256.
pub(super) fn sha_hex(sha: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    sha.iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Progress of a unit across its files, throttled to [`PROGRESS_EVERY`] (phase starts + file ends always told).
pub(super) struct Meter<'a> {
    emit: &'a dyn Fn(Progress),
    total: u64,
    /// Bytes of the unit's files before the current one.
    base: u64,
    last: Option<Instant>,
}

impl<'a> Meter<'a> {
    pub(super) fn new(total: u64, emit: &'a dyn Fn(Progress)) -> Self {
        Self {
            emit,
            total,
            base: 0,
            last: None,
        }
    }

    pub(super) fn next_file(&mut self, bytes: u64) {
        self.base = self.base.saturating_add(bytes);
    }

    /// `done` = bytes of the current file.
    fn tick(&mut self, done: u64, phase: Phase, force: bool) {
        let now = Instant::now();
        if force
            || self
                .last
                .is_none_or(|t| now.duration_since(t) >= PROGRESS_EVERY)
        {
            self.last = Some(now);
            (self.emit)(Progress {
                done: self.base.saturating_add(done),
                total: self.total,
                phase,
            });
        }
    }
}

/// What one fetch talks to.
pub(super) struct Fetch<'a> {
    pub(super) transport: &'a dyn Transport,
    pub(super) secret: Option<&'a Secret>,
    pub(super) cancel: &'a AtomicBool,
}

impl Fetch<'_> {
    fn cancelled(&self) -> Result<(), DownloadError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(DownloadError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// GET from byte `from` (`0`: whole file), redirects + allowlist + cancel per hop in [`open`].
    fn ask(&self, asset: &Asset, from: u64) -> Result<Response, DownloadError> {
        open(
            self.transport,
            asset,
            (from > 0).then_some(from),
            self.secret,
            self.cancel,
        )
    }
}

/// Bytes still to fetch for `asset` (free-space estimate, no hashing): target of pinned size -> 0; resumable part
/// -> the rest.
pub(super) fn remaining(dir: &Path, asset: &Asset) -> u64 {
    let files = Files::of(dir, asset);
    if file_len(&files.target) == Some(asset.bytes()) {
        return 0;
    }
    asset.bytes() - resumable(&files, asset).unwrap_or(0)
}

/// Length of a part its meta still pins, not past the pinned size.
fn resumable(files: &Files, asset: &Asset) -> Option<u64> {
    let len = file_len(&files.part)?;
    (PartMeta::read(&files.meta)? == PartMeta::of(asset) && len <= asset.bytes()).then_some(len)
}

/// Length of a regular file at `path` (a link | junction | folder: `None`; links never followed).
fn file_len(path: &Path) -> Option<u64> {
    std::fs::symlink_metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
}

/// Brings `asset` into `dir` verified. `Integrity` -> part + meta removed; any other failure keeps them (resume).
pub(super) fn fetch(
    cx: &Fetch<'_>,
    asset: &Asset,
    dir: &Path,
    meter: &mut Meter<'_>,
) -> Result<(), DownloadError> {
    let files = Files::of(dir, asset);
    let result = transfer(cx, asset, &files, meter);
    if result == Err(DownloadError::Integrity) {
        let _ = discard(&files);
    }
    result
}

fn transfer(
    cx: &Fetch<'_>,
    asset: &Asset,
    files: &Files,
    meter: &mut Meter<'_>,
) -> Result<(), DownloadError> {
    // a file moved in by an earlier, unfinished run of the unit: checked again, not fetched again
    if file_len(&files.target) == Some(asset.bytes()) {
        let mut hasher = Sha256::new();
        hash_file(cx, &files.target, &mut hasher, meter)?;
        if hasher.finalize().as_slice() == asset.sha256() {
            meter.tick(asset.bytes(), Phase::Verifying, true);
            return Ok(());
        }
    }
    let mut hasher = Sha256::new();
    let mut done = 0;
    if resumable(files, asset).is_some() {
        done = hash_file(cx, &files.part, &mut hasher, meter)?;
        if done > asset.bytes() {
            return Err(DownloadError::Integrity);
        }
    }
    // asked before any file is made: a refused first request leaves nothing
    let body = if done < asset.bytes() {
        Some(request(cx, asset, &mut hasher, &mut done)?)
    } else {
        None
    };
    let mut file = open_part(asset, files, done)?;
    if let Some(mut body) = body {
        stream(
            cx,
            asset,
            &mut *body,
            &mut file,
            &mut hasher,
            &mut done,
            meter,
        )?;
    }
    meter.tick(done, Phase::Verifying, true);
    if done != asset.bytes() || hasher.finalize().as_slice() != asset.sha256() {
        return Err(DownloadError::Integrity);
    }
    file.sync_all().map_err(|e| DownloadError::disk(&e))?;
    drop(file);
    fsx::replace_file(&files.part, &files.target).map_err(|e| DownloadError::disk(&e))?;
    // a meta left behind is swept as an orphan
    let _ = remove_if_there(&files.meta);
    Ok(())
}

/// Part cut to `done` bytes (the ones hashed, a regular file per [`resumable`]), opened at its end. `0` -> fresh meta
/// and part (meta first: a crash in between leaves a meta without part, swept); the fresh part is made new (a link |
/// a folder in its place: refused, never followed).
fn open_part(asset: &Asset, files: &Files, done: u64) -> Result<File, DownloadError> {
    let disk = |e: io::Error| DownloadError::disk(&e);
    let mut options = OpenOptions::new();
    options.write(true);
    if done == 0 {
        discard(files).map_err(disk)?;
        let meta =
            serde_json::to_vec(&PartMeta::of(asset)).map_err(|e| disk(io::Error::other(e)))?;
        fsx::write_atomic(&files.meta, &meta).map_err(disk)?;
        options.create_new(true);
    }
    let mut file = options.open(&files.part).map_err(disk)?;
    file.set_len(done).map_err(disk)?;
    file.seek(SeekFrom::Start(done)).map_err(disk)?;
    Ok(file)
}

/// Hashes the file at `path` (phase verifying); its length.
fn hash_file(
    cx: &Fetch<'_>,
    path: &Path,
    hasher: &mut Sha256,
    meter: &mut Meter<'_>,
) -> Result<u64, DownloadError> {
    let mut file = File::open(path).map_err(|e| DownloadError::disk(&e))?;
    let mut buf = vec![0; CHUNK];
    let mut done = 0;
    meter.tick(0, Phase::Verifying, true);
    loop {
        cx.cancelled()?;
        let n = match file.read(&mut buf) {
            Ok(0) => return Ok(done),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(DownloadError::disk(&e)),
        };
        hasher.update(&buf[..n]);
        done += n as u64;
        meter.tick(done, Phase::Verifying, false);
    }
}

/// Asks from `done`. `206` must start at `done` (another start | `416` to the range -> asked again from 0, once) and
/// total the pinned size; `200` -> restarted from 0 (`done` + `hasher` reset; the part is cut to `done` when opened).
/// A length other than the bytes still due -> `Integrity`.
fn request(
    cx: &Fetch<'_>,
    asset: &Asset,
    hasher: &mut Sha256,
    done: &mut u64,
) -> Result<Box<dyn Read + Send>, DownloadError> {
    let restart = |hasher: &mut Sha256, done: &mut u64| {
        *hasher = Sha256::new();
        *done = 0;
    };
    let mut response = match cx.ask(asset, *done) {
        // range not satisfiable (the source's file changed under the same pin, or a server quirk): from 0
        Err(DownloadError::Http { status: 416 }) if *done > 0 => {
            restart(hasher, done);
            cx.ask(asset, 0)?
        }
        other => other?,
    };
    if *done > 0 && response.status == 206 && content_range(&response).map(|r| r.0) != Some(*done) {
        restart(hasher, done);
        response = cx.ask(asset, 0)?;
    }
    if response.status == 206 {
        let (start, total) = content_range(&response).ok_or(DownloadError::Http { status: 206 })?;
        if total.is_some_and(|t| t != asset.bytes()) {
            return Err(DownloadError::Integrity);
        }
        if start != *done {
            return Err(DownloadError::Http { status: 206 });
        }
    } else if *done > 0 {
        // the whole file again: start over
        restart(hasher, done);
    }
    if response
        .content_length
        .is_some_and(|n| n != asset.bytes() - *done)
    {
        return Err(DownloadError::Integrity);
    }
    Ok(response.body)
}

/// `Content-Range: bytes S-E/T` -> `(S, Some(T))`; `T` = `*` -> `None`.
fn content_range(response: &Response) -> Option<(u64, Option<u64>)> {
    let spec = response.content_range.as_deref()?.trim();
    let spec = spec
        .get(..6)
        .filter(|unit| unit.eq_ignore_ascii_case("bytes "))
        .map(|_| &spec[6..])?;
    let (range, total) = spec.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    Some((start.trim().parse().ok()?, total))
}

/// Body -> part + hasher until the pinned size (never read past it). Cancel checked before each read; a body ending
/// early | failing -> `Offline` (part kept).
fn stream(
    cx: &Fetch<'_>,
    asset: &Asset,
    body: &mut dyn Read,
    file: &mut File,
    hasher: &mut Sha256,
    done: &mut u64,
    meter: &mut Meter<'_>,
) -> Result<(), DownloadError> {
    let mut buf = vec![0; CHUNK];
    meter.tick(*done, Phase::Downloading, true);
    while *done < asset.bytes() {
        cx.cancelled()?;
        let due = usize::try_from(asset.bytes() - *done).unwrap_or(usize::MAX);
        let n = match body.read(&mut buf[..due.min(CHUNK)]) {
            Ok(0) => return Err(DownloadError::Offline),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(DownloadError::Offline),
        };
        file.write_all(&buf[..n])
            .map_err(|e| DownloadError::disk(&e))?;
        hasher.update(&buf[..n]);
        *done += n as u64;
        meter.tick(*done, Phase::Downloading, false);
    }
    Ok(())
}

/// Removes part + meta of `asset`.
fn discard(files: &Files) -> io::Result<()> {
    remove_if_there(&files.part)?;
    remove_if_there(&files.meta)
}

/// Removes parts + metas of `assets` in `dir`.
pub(super) fn discard_all(dir: &Path, assets: &[Asset]) -> io::Result<()> {
    assets.iter().try_for_each(|a| discard(&Files::of(dir, a)))
}

/// Removes the file | link at `path` (link only, never its target), retrying transient Windows locks. Absent | a real
/// folder (not the store's: left): fine.
pub(super) fn remove_if_there(path: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    if meta.is_dir() {
        return Ok(());
    }
    // a Windows folder link | junction goes with `remove_dir` (the link itself)
    #[cfg(windows)]
    let remove = if std::os::windows::fs::FileTypeExt::is_symlink_dir(&meta.file_type()) {
        std::fs::remove_dir
    } else {
        std::fs::remove_file
    };
    #[cfg(not(windows))]
    let remove = std::fs::remove_file;
    match fsx::retry(&fsx::RETRY_DELAYS, || remove(path)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Sweep of `dir`: valid parts of `assets` -> `(index in assets, bytes there)`; removed: a part without meta, a meta
/// without part, a pair no asset pins (or longer than its pin), a linked part. Scratch names + folders untouched.
pub(super) fn sweep(dir: &Path, assets: &[&Asset]) -> io::Result<Vec<(usize, u64)>> {
    let entries = match std::fs::read_dir(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        other => other?,
    };
    let mut bases: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry?;
        // a real folder named like a part: not ours (links are: removed below)
        if entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if fsx::is_scratch_name(name) {
            continue;
        }
        let base = name
            .strip_suffix(".part.meta")
            .or_else(|| name.strip_suffix(".part"));
        if let Some(base) = base.filter(|b| !bases.iter().any(|seen| seen == b)) {
            bases.push(base.to_owned());
        }
    }
    let mut paused = Vec::new();
    for base in bases {
        let files = Files::named(dir, &base);
        let pinned = assets.iter().position(|a| a.name() == base);
        match pinned.and_then(|at| resumable(&files, assets[at]).map(|len| (at, len))) {
            Some(found) => paused.push(found),
            None => discard(&files)?,
        }
    }
    Ok(paused)
}
