//! Editing: the text an editor starts from, surgical saves, the person's copy on a conflict, pasted images.
//!
//! The editor sends line edits against the version it started from. A save checks that the disk still holds that
//! version (hash), applies the edits to the file's own text (kept lines keep their bytes and endings, the encoding
//! and BOM stay) and writes atomically, re-checking just before the rename. Every write is announced to the watch
//! as the app's own.

mod assets;
pub mod lines;

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod tests;

use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;
use ts_rs::TS;

pub use assets::{asset_kind, AssetKind, MAX_ASSET_BYTES};
pub use lines::LineEdit;

use crate::document::{DocScope, MAX_TEXT_BYTES};
use crate::error::{ErrorKind, IpcError};
use crate::format::config::{ignore_matches, IgnoreMatches, RowValue};
use crate::format::{
    segments, Body, ConfigEdit, EditRefusal, FormatId, FormatRegistry, OutlineItem, Segment,
};
use crate::fsx::{self, Encoding, FsxError, LineEnding};
use crate::paths::VaultPath;
use crate::watch::OwnWrites;

/// What an editor starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EditSource {
    pub path: String,
    /// The file's text, BOM gone, every line break as `\n`.
    pub text: String,
    /// xxh3 hex16 of the file bytes.
    pub hash: String,
    pub line_ending: LineEnding,
    pub encoding: Encoding,
    /// Block map (Markdown only).
    pub segments: Option<Vec<Segment>>,
}

/// The result of one value edit in a sheet: the text with only that value replaced, and its sheet.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConfigEdited {
    pub text: String,
    pub body: Body,
}

/// A secret value in the editor's text, in UTF-16 code units (what the editor counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SecretRange {
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SaveOutcome {
    Saved {
        hash: String,
        #[ts(type = "number")]
        mtime_ms: u64,
    },
    /// The file is not the version the edits are based on (`None`: it is gone). Nothing was written.
    Conflict { disk_hash: Option<String> },
}

/// Hears the saves of a workspace's editor (the Notes keep earlier versions with it). Both calls are best effort and
/// quick: a save never waits on more than a small file write.
pub trait SaveObserver: Send + Sync {
    /// The file `path` is about to be replaced by other bytes; `old` is what it holds now.
    fn replacing(&self, path: &str, old: &[u8]);
    /// The file `path` was replaced.
    fn replaced(&self, path: &str);
}

/// Editing in one workspace: its scope, its formats, the announcements of own writes (projects).
pub struct Editor<'a> {
    pub scope: &'a DocScope,
    pub formats: &'a FormatRegistry,
    pub own: Option<&'a OwnWrites>,
    pub observer: Option<&'a dyn SaveObserver>,
}

fn mtime_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn invalid(path: &str, detail: &str) -> IpcError {
    IpcError::new(ErrorKind::InvalidData)
        .subject(path)
        .detail(detail)
}

/// The decoded text of an editable file, or why it cannot be edited.
fn editable_text(path: &str, bytes: &[u8]) -> Result<fsx::Decoded, IpcError> {
    let decoded =
        fsx::decode(bytes).ok_or_else(|| IpcError::new(ErrorKind::Binary).subject(path))?;
    if decoded.lossy {
        return Err(IpcError::new(ErrorKind::Encoding).subject(path));
    }
    Ok(decoded)
}

/// What the disk holds now: `None` gone, else its hash.
fn disk_hash(abs: &Path) -> io::Result<Option<String>> {
    match fsx::retry(&fsx::RETRY_DELAYS, || std::fs::read(abs)) {
        Ok(bytes) => Ok(Some(fsx::hash(&bytes).hex())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

impl Editor<'_> {
    fn announce(&self, path: &str, bytes: &[u8]) -> Option<fsx::ContentHash> {
        let hash = fsx::hash(bytes);
        let own = self.own?;
        own.expect_write(path, hash);
        Some(hash)
    }

    fn retract(&self, path: &str, hash: Option<fsx::ContentHash>) {
        if let (Some(own), Some(hash)) = (self.own, hash) {
            own.retract_write(path, hash);
        }
    }

    /// The text an editor starts from.
    ///
    /// # Errors
    /// Outside the workspace | missing | too large | binary | lossy decode | no text format.
    pub fn source(&self, path: &str) -> Result<EditSource, IpcError> {
        let (_, abs) = self.scope.resolve(path)?;
        let bytes = fsx::read_bytes(&abs, MAX_TEXT_BYTES)?;
        let decoded = editable_text(path, &bytes)?;
        let head = &bytes[..bytes.len().min(4096)];
        let format = self
            .formats
            .pick(path, head)
            .filter(|f| f.reads_text())
            .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(path))?;
        let text = lines::normalized(&decoded.text);
        let segments =
            (format.id() == FormatId::MARKDOWN).then(|| segments(&text, self.formats.features()));
        Ok(EditSource {
            path: path.to_owned(),
            hash: fsx::hash(&bytes).hex(),
            line_ending: decoded.line_ending,
            encoding: decoded.encoding,
            text,
            segments,
        })
    }

    /// The block map of an unsaved Markdown text.
    #[must_use]
    pub fn segments(&self, text: &str) -> Vec<Segment> {
        segments(text, self.formats.features())
    }

    /// Applies `edits` (against the version `seen_hash`) to the file.
    ///
    /// # Errors
    /// Outside the workspace | read-only | too large | binary | lossy | bad edits | IO.
    pub fn save(
        &self,
        path: &str,
        seen_hash: &str,
        edits: &[LineEdit],
    ) -> Result<SaveOutcome, IpcError> {
        self.save_after(path, seen_hash, edits, || {})
    }

    /// [`Editor::save`] with `after_check` run between the first hash check and the write (tests let a writer in).
    fn save_after(
        &self,
        path: &str,
        seen_hash: &str,
        edits: &[LineEdit],
        after_check: impl FnOnce(),
    ) -> Result<SaveOutcome, IpcError> {
        let (_, abs) = self.scope.resolve(path)?;
        let meta = match std::fs::metadata(&abs) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => return Err(IpcError::new(ErrorKind::NotFound).subject(path)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(SaveOutcome::Conflict { disk_hash: None })
            }
            Err(e) => return Err(IpcError::io(path, &e)),
        };
        if meta.permissions().readonly() {
            return Err(IpcError::new(ErrorKind::Denied)
                .subject(path)
                .detail("readOnly"));
        }
        let bytes = match fsx::read_bytes(&abs, MAX_TEXT_BYTES) {
            Ok(bytes) => bytes,
            Err(FsxError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(SaveOutcome::Conflict { disk_hash: None })
            }
            Err(e) => return Err(e.into()),
        };
        let disk = fsx::hash(&bytes).hex();
        if disk != seen_hash {
            return Ok(SaveOutcome::Conflict {
                disk_hash: Some(disk),
            });
        }
        let decoded = editable_text(path, &bytes)?;
        let text = lines::apply(&decoded.text, edits).map_err(|problem| {
            invalid(
                path,
                match problem {
                    lines::EditProblem::OutOfRange => "outOfRange",
                    lines::EditProblem::Order => "order",
                    lines::EditProblem::LineBreak => "lineBreak",
                },
            )
        })?;
        let out = lines::encode(&text, decoded.encoding);
        if out.len() as u64 > MAX_TEXT_BYTES {
            return Err(IpcError::new(ErrorKind::TooLarge).subject(path));
        }
        after_check();
        if out == bytes {
            // nothing to write, nothing for the watch to see
            return Ok(SaveOutcome::Saved {
                hash: disk,
                mtime_ms: mtime_ms(&abs),
            });
        }
        if let Some(observer) = self.observer {
            observer.replacing(path, &bytes);
        }
        let announced = self.announce(path, &out);
        let mut gone = false;
        let mut other = None;
        let written = fsx::write_atomic_checked(&abs, &out, || match disk_hash(&abs)? {
            Some(now) if now == seen_hash => Ok(true),
            Some(now) => {
                other = Some(now);
                Ok(false)
            }
            None => {
                gone = true;
                Ok(false)
            }
        });
        match written {
            Ok(true) => {
                if let Some(observer) = self.observer {
                    observer.replaced(path);
                }
                Ok(SaveOutcome::Saved {
                    hash: fsx::hash(&out).hex(),
                    mtime_ms: mtime_ms(&abs),
                })
            }
            Ok(false) => {
                self.retract(path, announced);
                Ok(SaveOutcome::Conflict {
                    disk_hash: if gone { None } else { other },
                })
            }
            Err(e) => {
                self.retract(path, announced);
                Err(IpcError::io(path, &e))
            }
        }
    }

    /// Writes the person's text as a new file: `path` itself when `exact` (a deleted document written back), else
    /// `<stem>-mine.<ext>` next to it (`-mine-2`, ...). Line ending + encoding of the original when it is there.
    /// Never overwrites. Returns the new file's path.
    ///
    /// # Errors
    /// Outside the workspace | `exact` and the file exists | IO.
    pub fn save_as(&self, path: &str, text: &str, exact: bool) -> Result<String, IpcError> {
        let parsed = VaultPath::parse(path)?;
        let (_, original) = self.scope.resolve_new(path)?;
        let (ending, encoding) = match fsx::read_text(&original, MAX_TEXT_BYTES) {
            Ok(decoded) => (
                lines::majority(&lines::split(&decoded.text)).to_owned(),
                decoded.encoding,
            ),
            Err(_) => ("\n".to_owned(), Encoding::Utf8),
        };
        let body: String = if ending == "\n" {
            text.to_owned()
        } else {
            text.replace('\n', &ending)
        };
        let bytes = lines::encode(&body, encoding);
        if bytes.len() as u64 > MAX_TEXT_BYTES {
            return Err(IpcError::new(ErrorKind::TooLarge).subject(path));
        }
        let candidates: Box<dyn Iterator<Item = VaultPath>> = if exact {
            Box::new(std::iter::once(parsed))
        } else {
            let name = parsed.name().unwrap_or("document.md");
            let (stem, ext) = split_name(name);
            let parent = parsed.parent().unwrap_or_else(VaultPath::root);
            Box::new((1..=999_u32).map(move |n| {
                let suffix = if n == 1 {
                    "-mine".to_owned()
                } else {
                    format!("-mine-{n}")
                };
                parent.join_name(&format!("{stem}{suffix}{ext}"))
            }))
        };
        for candidate in candidates {
            let target = candidate.as_str().to_owned();
            let (_, abs) = self.scope.resolve_new(&target)?;
            if abs.exists() {
                if exact {
                    return Err(IpcError::new(ErrorKind::Exists).subject(target));
                }
                continue;
            }
            let announced = self.announce(&target, &bytes);
            match assets::write_new(&abs, &bytes) {
                Ok(()) => return Ok(target),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && !exact => {
                    self.retract(&target, announced);
                }
                Err(e) => {
                    self.retract(&target, announced);
                    return Err(IpcError::io(target, &e));
                }
            }
        }
        Err(IpcError::new(ErrorKind::Exists).subject(path))
    }

    /// Saves pasted | dropped picture bytes next to the document `doc` as `<doc-stem>-<n>.<ext>`.
    ///
    /// # Errors
    /// Unknown type | bytes not of that type (`unsupported`) | over 50 MB (`tooLarge`) | outside | IO.
    pub fn write_asset(&self, doc: &str, ext: &str, bytes: &[u8]) -> Result<String, IpcError> {
        let kind = asset_kind(ext).ok_or_else(|| {
            IpcError::new(ErrorKind::Unsupported)
                .subject(doc)
                .detail(ext.to_owned())
        })?;
        if bytes.len() as u64 > MAX_ASSET_BYTES {
            return Err(IpcError::new(ErrorKind::TooLarge).subject(doc));
        }
        if !kind.matches(bytes) {
            return Err(IpcError::new(ErrorKind::Unsupported)
                .subject(doc)
                .detail("content"));
        }
        let parsed = VaultPath::parse(doc)?;
        let (stem, _) = split_name(parsed.name().unwrap_or("image"));
        let parent = parsed.parent().unwrap_or_else(VaultPath::root);
        for n in 1..=9999_u32 {
            let target = parent.join_name(&format!("{stem}-{n}.{}", kind.ext()));
            let target = target.as_str().to_owned();
            let (_, abs) = self.scope.resolve_new(&target)?;
            if abs.exists() {
                continue;
            }
            let announced = self.announce(&target, bytes);
            match assets::write_new(&abs, bytes) {
                Ok(()) => return Ok(target),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    self.retract(&target, announced);
                }
                Err(e) => {
                    self.retract(&target, announced);
                    return Err(IpcError::io(target, &e));
                }
            }
        }
        Err(IpcError::new(ErrorKind::Exists).subject(doc))
    }

    /// The format of `path` read from the editor's `text`.
    fn config_format(
        &self,
        path: &str,
        text: &str,
    ) -> Result<&std::sync::Arc<dyn crate::format::Format>, IpcError> {
        let head = &text.as_bytes()[..text.len().min(4096)];
        self.formats
            .pick(path, head)
            .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(path))
    }

    /// The sheet (or the text fallback) of an unsaved text of a config file.
    ///
    /// # Errors
    /// No format reads `path`.
    pub fn config_sheet(&self, path: &str, text: &str) -> Result<Body, IpcError> {
        let format = self.config_format(path, text)?;
        let known = std::collections::HashSet::new();
        let cx = crate::format::RenderCx {
            path,
            env: self.scope,
            features: self.formats.features(),
            known: &known,
        };
        Ok(format.render(text, &cx).body)
    }

    /// One value edit on the editor's text: only that value's bytes change.
    ///
    /// # Errors
    /// The format cannot make the edit (`invalidData`, `detail` = why: a read-only reason, `notFound`, `syntax`,
    /// `roundTrip`, ...).
    pub fn config_edit(
        &self,
        path: &str,
        text: &str,
        edit: &ConfigEdit,
    ) -> Result<ConfigEdited, IpcError> {
        let format = self.config_format(path, text)?;
        let splice = format.edit(text, edit).map_err(|refusal| {
            let kind = match refusal {
                EditRefusal::NotFound => ErrorKind::NotFound,
                EditRefusal::Unsupported => ErrorKind::Unsupported,
                EditRefusal::ReadOnly(_) | EditRefusal::Invalid(_) => ErrorKind::InvalidData,
            };
            IpcError::new(kind).subject(path).detail(refusal.word())
        })?;
        let new = splice.apply(text);
        let body = self.config_sheet(path, &new)?;
        Ok(ConfigEdited { text: new, body })
    }

    /// Where the secret values of an unsaved text are, in UTF-16 units.
    ///
    /// # Errors
    /// No format reads `path`.
    pub fn config_secrets(&self, path: &str, text: &str) -> Result<Vec<SecretRange>, IpcError> {
        let format = self.config_format(path, text)?;
        let spans = format.secrets(text);
        let mut out = Vec::with_capacity(spans.len());
        let mut units = 0_u32;
        let mut byte = 0_usize;
        for span in spans {
            // spans come in file order; count UTF-16 units between them
            if span.start < byte || span.end > text.len() {
                continue;
            }
            let from = units + utf16_len(&text[byte..span.start]);
            let to = from + utf16_len(&text[span.start..span.end]);
            out.push(SecretRange { from, to });
            units = to;
            byte = span.end;
        }
        Ok(out)
    }

    /// How many project files each pattern of an ignore file matches (Project workspaces only). `text`: the editor's
    /// version; none = the file as it is on disk.
    ///
    /// # Errors
    /// Not a Project workspace | no format reads `path` | outside | unreadable.
    pub fn ignore_matches(
        &self,
        path: &str,
        text: Option<&str>,
    ) -> Result<IgnoreMatches, IpcError> {
        if !matches!(self.scope, DocScope::Project(_)) {
            return Err(IpcError::new(ErrorKind::Unsupported).subject(path));
        }
        let (_, abs) = self.scope.resolve(path)?;
        let base = abs
            .parent()
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(path))?;
        let disk;
        let text = if let Some(text) = text {
            text
        } else {
            disk = fsx::read_text(&abs, MAX_TEXT_BYTES)?.text;
            &disk
        };
        let format = self.config_format(path, text)?;
        let patterns: Vec<(u32, String)> = format
            .sheet(text)
            .map(|sheet| {
                sheet
                    .rows()
                    .filter_map(|(_, row)| match &row.value {
                        RowValue::Pattern { text, .. } => Some((row.line, text.clone())),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(ignore_matches(base, &patterns))
    }

    /// Headings of a Markdown document (link completion after `#`): anchor ids as the reading view makes them.
    ///
    /// # Errors
    /// Outside | missing | unreadable.
    pub fn headings(&self, path: &str) -> Result<Vec<OutlineItem>, IpcError> {
        let (_, abs) = self.scope.resolve(path)?;
        let decoded = fsx::read_text(&abs, MAX_TEXT_BYTES)?;
        let known = std::collections::HashSet::new();
        let cx = crate::format::RenderCx {
            path,
            env: self.scope,
            features: self.formats.features(),
            known: &known,
        };
        match crate::format::Format::render(&crate::format::MarkdownFormat, &decoded.text, &cx).body
        {
            crate::format::Body::Markdown { outline, .. } => Ok(outline),
            _ => Ok(Vec::new()),
        }
    }
}

fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// `name` -> (stem, `.ext` | ""). A dotfile has no extension.
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    }
}
