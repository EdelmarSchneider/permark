//! Editing documents: the source for the editor, safe saves, new files and pasted images.
//!
//! Everything here is plain Rust over a [`Vault`], so it is tested without a window. Paths from the UI
//! go through [`Vault::resolve`] first. Saves are atomic and compare the file on disk with the hash the
//! editor started from, so an agent's newer version is never overwritten without the person knowing.
//! Every write is announced to [`OwnWrites`], so the watcher does not report it back as news.

mod assets;
pub mod blocks;
#[cfg(test)]
mod corpus;

use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

pub use assets::{decode_image, AssetWritten};
pub use blocks::{parse_blocks, ParsedBlocks};

use crate::render::markdown::{is_markdown_path, render_markdown};
use crate::render::MAX_RENDER_BYTES;
use crate::util::{
    hash_bytes, hash_hex, modified_ms, read_with_retry, write_atomic_guarded, Guarded,
};
use crate::vault::{Vault, VaultError};
use crate::watch::OwnWrites;

/// Names Windows reserves for devices; a file cannot have one of them as its stem on any system we ship to.
const RESERVED_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("{0} is a binary file, so it cannot be edited")]
    Binary(String),
    #[error("{0} is too large to edit")]
    TooLarge(String),
    #[error("{0} is not a Markdown file")]
    NotMarkdown(String),
    #[error("{0} is not UTF-8 text, so it cannot be edited here")]
    Encoding(String),
    #[error("a file named {0} already exists")]
    Exists(String),
    #[error("{0}")]
    InvalidName(NameProblem),
    #[error("{0}")]
    Unsupported(String),
    #[error("the image data is not valid: {0}")]
    InvalidData(String),
    #[error("cannot write {path}: {source}")]
    Io { path: String, source: io::Error },
}

/// Why a name cannot be used. The English text is for logs; the UI shows its own words for [`NameProblem::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameProblem {
    Empty,
    ForbiddenChars,
    EdgeSpaceOrDot,
    NoStem,
    Reserved,
    TooLong,
    NotImage,
    EmptyImage,
}

impl NameProblem {
    /// A short stable identifier for the UI.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::ForbiddenChars => "forbiddenChars",
            Self::EdgeSpaceOrDot => "edgeSpaceOrDot",
            Self::NoStem => "noStem",
            Self::Reserved => "reserved",
            Self::TooLong => "tooLong",
            Self::NotImage => "notImage",
            Self::EmptyImage => "emptyImage",
        }
    }
}

impl std::fmt::Display for NameProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "The name is empty",
            Self::ForbiddenChars => "Names can't contain \\ / : * ? \" < > |",
            Self::EdgeSpaceOrDot => "Names can't start with a space or end with a space or a dot",
            Self::NoStem => "The name needs something before the extension",
            Self::Reserved => "That name is reserved by the system",
            Self::TooLong => "The name is too long",
            Self::NotImage => "Only images can be added here (PNG, JPG, GIF, WebP, SVG)",
            Self::EmptyImage => "The image is empty",
        })
    }
}

impl EditError {
    /// A short stable identifier for the UI.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Vault(e) => e.kind(),
            Self::Binary(_) => "binary",
            Self::TooLarge(_) => "tooLarge",
            Self::NotMarkdown(_) => "notMarkdown",
            Self::Encoding(_) => "encoding",
            Self::Exists(_) => "exists",
            Self::InvalidName(_) => "invalidName",
            Self::Unsupported(_) => "unsupported",
            Self::InvalidData(_) => "invalidData",
            Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied => "denied",
            Self::Io { .. } => "io",
        }
    }

    fn io(path: &str, source: io::Error) -> Self {
        if source.kind() == io::ErrorKind::NotFound {
            Self::Vault(VaultError::NotFound(path.to_owned()))
        } else {
            Self::Io {
                path: path.to_owned(),
                source,
            }
        }
    }
}

/// A document ready for the editor. Matches the UI's `SourceDoc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDoc {
    pub path: String,
    /// The file's text exactly as it is on disk (a BOM is the first character, line endings are kept).
    pub text: String,
    /// Hash of the bytes on disk; `save_document` compares it with the file to detect a newer version.
    pub hash: String,
    #[serde(flatten)]
    pub parsed: ParsedBlocks,
    pub modified_ms: u64,
}

/// The result of a save. Matches the UI's `SaveOutcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SaveOutcome {
    Saved {
        hash: String,
        modified_ms: u64,
    },
    /// The file is not the version the editor started from. `disk_hash` is the hash of what is there now,
    /// or `None` when the file is gone.
    Conflict {
        disk_hash: Option<String>,
    },
}

/// A file made by [`Editor::create`]. Matches the UI's `CreatedDoc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedDoc {
    pub path: String,
    pub hash: String,
    pub modified_ms: u64,
}

/// Reads, saves and creates the files of one vault.
pub struct Editor<'a> {
    vault: &'a Vault,
    own: &'a OwnWrites,
    /// Plain text files (`.txt`) are edited too, not only Markdown.
    text_files: bool,
}

impl<'a> Editor<'a> {
    pub fn new(vault: &'a Vault, own: &'a OwnWrites) -> Self {
        Self {
            vault,
            own,
            text_files: false,
        }
    }

    /// An editor that also reads and saves plain text files (`.txt`), which a project's editor leaves alone.
    #[must_use]
    pub fn with_text_files(mut self) -> Self {
        self.text_files = true;
        self
    }

    /// Markdown by file name, or plain text when this editor takes it (a file that holds binary data is caught by the
    /// content checks).
    fn check_kind(&self, abs: &Path, rel: &str) -> Result<(), EditError> {
        if is_markdown_path(abs) || (self.text_files && is_plain_text_path(abs)) {
            Ok(())
        } else {
            Err(EditError::NotMarkdown(rel.to_owned()))
        }
    }

    /// The text of a Markdown file and its blocks. Binary files, files over the size limit, files that are
    /// not UTF-8 (rewriting them would change their encoding) and non-Markdown files are refused.
    pub fn source(&self, path: &str) -> Result<SourceDoc, EditError> {
        let resolved = self.vault.resolve(path)?;
        let rel = resolved.rel;
        self.check_kind(&resolved.abs, &rel)?;
        let meta = std::fs::metadata(&resolved.abs).map_err(|e| EditError::io(&rel, e))?;
        if !meta.is_file() {
            return Err(VaultError::NotFound(rel).into());
        }
        if meta.len() > MAX_RENDER_BYTES {
            return Err(EditError::TooLarge(rel));
        }
        let bytes = read_with_retry(&resolved.abs).map_err(|e| EditError::io(&rel, e))?;
        let hash = hash_hex(&bytes);
        let text = text_of(bytes, &rel)?;
        Ok(SourceDoc {
            parsed: parse_blocks(&text),
            path: rel,
            text,
            hash,
            modified_ms: modified_ms(&meta),
        })
    }

    /// Writes `text` over the file when it still is the version with hash `expected_hash`. A file that
    /// changed or vanished meanwhile is reported as a conflict and left alone.
    ///
    /// The comparison is made twice: first to answer a stale save at once, and again after the new bytes
    /// are written and flushed to a temp file, right before it is renamed over the file (see
    /// [`write_atomic_guarded`]), because writing takes long enough for an agent to save in between.
    /// A save of the text that is already on disk writes nothing.
    pub fn save(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
    ) -> Result<SaveOutcome, EditError> {
        self.save_after(path, text, expected_hash, || {})
    }

    /// [`Editor::save`] with `after_check` run between the first comparison and the write: the moment in
    /// which a test makes another writer get in.
    fn save_after(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
        after_check: impl FnOnce(),
    ) -> Result<SaveOutcome, EditError> {
        let resolved = self.vault.resolve(path)?;
        let rel = resolved.rel;
        self.check_kind(&resolved.abs, &rel)?;
        check_size(text, &rel)?;
        let meta = match std::fs::metadata(&resolved.abs) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => return Err(VaultError::NotFound(rel).into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(SaveOutcome::Conflict { disk_hash: None });
            }
            Err(e) => return Err(EditError::io(&rel, e)),
        };
        if meta.permissions().readonly() {
            let denied = io::Error::new(io::ErrorKind::PermissionDenied, "the file is read-only");
            return Err(EditError::io(&rel, denied));
        }
        if meta.len() > MAX_RENDER_BYTES {
            return Err(EditError::TooLarge(rel));
        }
        match conflict_with_disk(&resolved.abs, expected_hash) {
            Ok(Some(conflict)) => return Ok(conflict),
            Ok(None) => {}
            Err(e) => return Err(EditError::io(&rel, e)),
        }
        after_check();

        let bytes = text.as_bytes();
        let hash = hash_bytes(bytes);
        let hash_text = format!("{hash:016x}");
        if hash_text == expected_hash {
            // The file holds exactly this text already: nothing to write, and nothing for the watcher to see.
            return Ok(SaveOutcome::Saved {
                hash: hash_text,
                modified_ms: modified_ms(&meta),
            });
        }
        self.own.expect(&rel, hash);
        let written = write_atomic_guarded(&resolved.abs, bytes, || {
            conflict_with_disk(&resolved.abs, expected_hash)
        });
        match written {
            Ok(Guarded::Written) => {}
            Ok(Guarded::Refused(conflict)) => {
                self.own.retract(&rel, hash);
                return Ok(conflict);
            }
            Err(e) => {
                self.own.retract(&rel, hash);
                return Err(EditError::io(&rel, e));
            }
        }
        let modified = std::fs::metadata(&resolved.abs).map_or(0, |m| modified_ms(&m));
        Ok(SaveOutcome::Saved {
            hash: hash_text,
            modified_ms: modified,
        })
    }

    /// Creates a new Markdown file. Fails if the name is not usable or the file exists.
    pub fn create(&self, path: &str, text: &str) -> Result<CreatedDoc, EditError> {
        let resolved = self.vault.resolve(path)?;
        let rel = resolved.rel;
        self.check_kind(&resolved.abs, &rel)?;
        check_size(text, &rel)?;
        let name = resolved
            .abs
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| VaultError::Invalid(rel.clone()))?;
        validate_file_name(&name).map_err(EditError::InvalidName)?;

        let bytes = text.as_bytes();
        let hash = hash_bytes(bytes);
        self.own.expect(&rel, hash);
        let written = write_new_file(&resolved.abs, bytes);
        if let Err(e) = written {
            self.own.retract(&rel, hash);
            return Err(if e.kind() == io::ErrorKind::AlreadyExists {
                EditError::Exists(name)
            } else {
                EditError::io(&rel, e)
            });
        }
        let modified = std::fs::metadata(&resolved.abs).map_or(0, |m| modified_ms(&m));
        Ok(CreatedDoc {
            path: rel,
            hash: format!("{hash:016x}"),
            modified_ms: modified,
        })
    }

    /// Writes an image next to a document, in its `assets` folder (see [`assets`]).
    pub fn write_asset(
        &self,
        doc_path: &str,
        name_hint: &str,
        data: &[u8],
    ) -> Result<AssetWritten, EditError> {
        assets::write(self.vault, self.own, doc_path, name_hint, data)
    }
}

/// The reading view's HTML for some pieces of Markdown, one string for each piece, in the same order. Visual mode draws its locked
/// blocks (raw HTML, math, diagrams, alerts...) with this. The same renderer and sanitizer as the reading view; a piece that does not
/// fit in what is left of the size limit comes back empty (the UI then shows its source).
pub fn render_fragments(texts: &[String]) -> Vec<String> {
    let mut left = usize::try_from(MAX_RENDER_BYTES).unwrap_or(usize::MAX);
    texts
        .iter()
        .map(|text| {
            if text.len() > left {
                return String::new();
            }
            left -= text.len();
            render_markdown(text).html
        })
        .collect()
}

/// How the file on disk compares with the version a save is based on: `None` when it is that version,
/// else the conflict to report (the file's hash, or none when it is gone).
fn conflict_with_disk(abs: &Path, expected_hash: &str) -> io::Result<Option<SaveOutcome>> {
    match read_with_retry(abs) {
        Ok(on_disk) => {
            let disk_hash = hash_hex(&on_disk);
            Ok(
                (disk_hash != expected_hash).then_some(SaveOutcome::Conflict {
                    disk_hash: Some(disk_hash),
                }),
            )
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Ok(Some(SaveOutcome::Conflict { disk_hash: None }))
        }
        Err(e) => Err(e),
    }
}

/// Whether the file name says plain text (`.txt`).
pub(crate) fn is_plain_text_path(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        matches!(e.as_str(), "txt" | "text")
    })
}

fn check_size(text: &str, rel: &str) -> Result<(), EditError> {
    if text.len() as u64 > MAX_RENDER_BYTES {
        Err(EditError::TooLarge(rel.to_owned()))
    } else {
        Ok(())
    }
}

/// The bytes of a file as text, or the reason it cannot be edited.
fn text_of(bytes: Vec<u8>, rel: &str) -> Result<String, EditError> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(EditError::Encoding(rel.to_owned()));
    }
    if bytes[..bytes.len().min(8 * 1024)].contains(&0) {
        return Err(EditError::Binary(rel.to_owned()));
    }
    String::from_utf8(bytes).map_err(|_| EditError::Encoding(rel.to_owned()))
}

/// Creates a file that must not exist yet and writes all of it.
fn write_new_file(abs: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(abs)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = std::fs::remove_file(abs);
    } else {
        crate::util::sync_parent(abs);
    }
    written
}

/// Whether `name` can be the name of a new file on every system we run on.
pub(crate) fn validate_file_name(name: &str) -> Result<(), NameProblem> {
    const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    let stem = name.split('.').next().unwrap_or_default();
    // Windows also treats `con .md` as the device: spaces before the extension do not count.
    let device_stem = stem.trim_end();
    if name.is_empty() || name == "." || name == ".." {
        Err(NameProblem::Empty)
    } else if name
        .chars()
        .any(|c| FORBIDDEN.contains(&c) || c.is_control())
    {
        Err(NameProblem::ForbiddenChars)
    } else if name.ends_with('.') || name.ends_with(' ') || name.starts_with(' ') {
        Err(NameProblem::EdgeSpaceOrDot)
    } else if stem.is_empty() {
        Err(NameProblem::NoStem)
    } else if RESERVED_NAMES.contains(&device_stem.to_ascii_lowercase().as_str()) {
        Err(NameProblem::Reserved)
    } else if name.len() > 255 {
        Err(NameProblem::TooLong)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct Fx {
        _dir: TempDir,
        vault: Arc<Vault>,
        own: OwnWrites,
    }

    impl Fx {
        fn new(files: &[(&str, &[u8])]) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            for (name, content) in files {
                let p = dir.path().join(name);
                fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
                fs::write(&p, content).expect("write");
            }
            let vault = Arc::new(Vault::open(dir.path()).expect("open"));
            Self {
                _dir: dir,
                vault,
                own: OwnWrites::new(),
            }
        }

        fn editor(&self) -> Editor<'_> {
            Editor::new(&self.vault, &self.own)
        }

        fn read(&self, rel: &str) -> Vec<u8> {
            fs::read(self.vault.root().join(rel)).expect("read")
        }
    }

    fn kind_of<T: std::fmt::Debug>(r: Result<T, EditError>) -> &'static str {
        r.expect_err("an error").kind()
    }

    #[test]
    fn the_source_is_the_text_on_disk_with_its_hash_and_blocks() {
        let bytes = "\u{FEFF}# Olá\r\n\r\ntexto\r\n".as_bytes();
        let fx = Fx::new(&[("a.md", bytes)]);
        let doc = fx.editor().source("a.md").expect("source");
        assert_eq!(doc.text.as_bytes(), bytes, "BOM and CRLF are kept");
        assert_eq!(doc.hash, hash_hex(bytes));
        assert_eq!(doc.path, "a.md");
        assert_eq!(doc.parsed.blocks.len(), 2);
        assert_eq!(doc.parsed.eol, blocks::Eol::Crlf);
        assert!(doc.modified_ms > 0);
        let json = serde_json::to_value(&doc).expect("json");
        for field in ["path", "text", "hash", "eol", "blocks", "modifiedMs"] {
            assert!(json.get(field).is_some(), "missing {field}");
        }
        assert_eq!(json["blocks"][0]["kind"], "heading");
        assert_eq!(json["blocks"][0]["startByte"], 3);
    }

    #[test]
    fn files_that_cannot_be_edited_are_refused_with_the_reason() {
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("# Hi\n".encode_utf16().flat_map(u16::to_le_bytes));
        let fx = Fx::new(&[
            ("bin.md", b"PK\x03\x04\0\0abc"),
            ("latin.md", b"# Caf\xE9\n"),
            ("utf16.md", &utf16),
            ("notes.txt", b"plain"),
            ("ok.md", b"fine\n"),
        ]);
        let ed = fx.editor();
        assert_eq!(kind_of(ed.source("bin.md")), "binary");
        assert_eq!(kind_of(ed.source("latin.md")), "encoding");
        assert_eq!(kind_of(ed.source("utf16.md")), "encoding");
        assert_eq!(kind_of(ed.source("notes.txt")), "notMarkdown");
        assert_eq!(kind_of(ed.source("missing.md")), "notFound");
        assert_eq!(kind_of(ed.source("../outside.md")), "pathEscape");
        assert_eq!(kind_of(ed.source("")), "notMarkdown");
        assert!(ed.source("ok.md").is_ok());
    }

    #[test]
    fn a_file_over_the_limit_is_too_large_to_edit() {
        let fx = Fx::new(&[]);
        let big = fx.vault.root().join("big.md");
        fs::File::create(&big)
            .and_then(|f| f.set_len(MAX_RENDER_BYTES + 1))
            .expect("big file");
        assert_eq!(kind_of(fx.editor().source("big.md")), "tooLarge");
        let ok = fx.editor().save("big.md", "x", "0000000000000000");
        assert_eq!(kind_of(ok), "tooLarge");
        let huge = "x".repeat(usize::try_from(MAX_RENDER_BYTES).expect("fits") + 1);
        fs::write(fx.vault.root().join("a.md"), "a").expect("write");
        assert_eq!(
            kind_of(fx.editor().save("a.md", &huge, &hash_hex(b"a"))),
            "tooLarge"
        );
    }

    #[test]
    fn saving_writes_the_text_as_it_is_and_returns_the_new_hash() {
        let fx = Fx::new(&[("a.md", b"# One\n")]);
        let ed = fx.editor();
        let before = ed.source("a.md").expect("source");
        let text = "\u{FEFF}# Two\r\n\r\nübung\r\n";
        let out = ed.save("a.md", text, &before.hash).expect("save");
        let SaveOutcome::Saved { hash, modified_ms } = out else {
            panic!("expected Saved, got {out:?}");
        };
        assert_eq!(fx.read("a.md"), text.as_bytes());
        assert_eq!(hash, hash_hex(text.as_bytes()));
        assert!(modified_ms > 0);
        assert_eq!(ed.source("a.md").expect("again").hash, hash);
        // Saving again from the new hash works; there are no temp files left.
        assert!(matches!(
            ed.save("a.md", "# Three\n", &hash),
            Ok(SaveOutcome::Saved { .. })
        ));
        let mut names: Vec<String> = fs::read_dir(fx.vault.root())
            .expect("ls")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a.md"]);
    }

    #[test]
    fn a_file_changed_behind_the_editor_is_a_conflict_and_is_left_alone() {
        let fx = Fx::new(&[("a.md", b"# One\n")]);
        let ed = fx.editor();
        let before = ed.source("a.md").expect("source");
        fs::write(fx.vault.root().join("a.md"), "# By an agent\n").expect("agent write");

        let out = ed.save("a.md", "# Mine\n", &before.hash).expect("save");
        assert_eq!(
            out,
            SaveOutcome::Conflict {
                disk_hash: Some(hash_hex(b"# By an agent\n"))
            }
        );
        assert_eq!(fx.read("a.md"), b"# By an agent\n", "nothing was written");

        // "Keep mine": the hash on disk is the expected one, the save goes through.
        let disk = hash_hex(b"# By an agent\n");
        assert!(matches!(
            ed.save("a.md", "# Mine\n", &disk),
            Ok(SaveOutcome::Saved { .. })
        ));
        assert_eq!(fx.read("a.md"), b"# Mine\n");
    }

    fn names_in(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(root)
            .expect("ls")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Whether the watcher would hear about a change of `rel` to `content` (it is not one of our own writes).
    fn is_news(fx: &Fx, rel: &str, content: &[u8]) -> bool {
        let sigs = crate::watch::SigStore::new();
        sigs.set(
            crate::vault::key(rel),
            rel,
            crate::watch::FileSig {
                len: content.len() as u64,
                mtime_ms: 1,
                hash: hash_bytes(content),
            },
        );
        let mut changes = vec![crate::watch::FsChange {
            kind: crate::watch::ChangeKind::Modified,
            path: rel.to_owned(),
            from: None,
            at_ms: 1,
        }];
        fx.own.strip(&mut changes, &sigs);
        !changes.is_empty()
    }

    #[test]
    fn a_writer_that_gets_in_after_the_first_check_is_not_overwritten() {
        let fx = Fx::new(&[("a.md", b"# One\n")]);
        let ed = fx.editor();
        let before = ed.source("a.md").expect("source");
        let path = fx.vault.root().join("a.md");
        // The agent saves while our new bytes are being written and flushed to the temp file.
        let out = ed
            .save_after("a.md", "# Mine\n", &before.hash, || {
                fs::write(&path, "# By an agent\n").expect("agent write");
            })
            .expect("save");
        assert_eq!(
            out,
            SaveOutcome::Conflict {
                disk_hash: Some(hash_hex(b"# By an agent\n"))
            }
        );
        assert_eq!(
            fx.read("a.md"),
            b"# By an agent\n",
            "the agent's work stays"
        );
        assert_eq!(names_in(fx.vault.root()), ["a.md"], "no temp file is left");
        assert!(
            is_news(&fx, "a.md", b"# Mine\n"),
            "a refused save must not be announced as ours"
        );
    }

    #[test]
    fn a_file_that_vanishes_after_the_first_check_is_not_recreated() {
        let fx = Fx::new(&[("a.md", b"# One\n")]);
        let ed = fx.editor();
        let before = ed.source("a.md").expect("source");
        let path = fx.vault.root().join("a.md");
        let out = ed
            .save_after("a.md", "# Mine\n", &before.hash, || {
                fs::remove_file(&path).expect("rm");
            })
            .expect("save");
        assert_eq!(out, SaveOutcome::Conflict { disk_hash: None });
        assert_eq!(names_in(fx.vault.root()), Vec::<String>::new());
    }

    #[test]
    fn saving_the_text_that_is_already_there_writes_and_announces_nothing() {
        let fx = Fx::new(&[("a.md", b"# Same\n")]);
        let path = fx.vault.root().join("a.md");
        let ed = fx.editor();
        let before = ed.source("a.md").expect("source");
        let stamp = fs::metadata(&path)
            .expect("meta")
            .modified()
            .expect("mtime");
        let out = ed.save("a.md", "# Same\n", &before.hash).expect("save");
        assert_eq!(
            out,
            SaveOutcome::Saved {
                hash: before.hash.clone(),
                modified_ms: before.modified_ms
            }
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("meta")
                .modified()
                .expect("mtime"),
            stamp,
            "the file was not rewritten"
        );
        assert!(
            is_news(&fx, "a.md", b"# Same\n"),
            "nothing is left announced for a write that never happened"
        );
        // and an agent changing it later is not hidden by a leftover announcement
        fs::write(&path, "# Agent\n").expect("write");
        assert!(is_news(&fx, "a.md", b"# Agent\n"));
    }

    #[test]
    fn a_file_deleted_behind_the_editor_is_a_conflict_without_a_disk_hash() {
        let fx = Fx::new(&[("a.md", b"x")]);
        let ed = fx.editor();
        let hash = hash_hex(b"x");
        fs::remove_file(fx.vault.root().join("a.md")).expect("rm");
        assert_eq!(
            ed.save("a.md", "y", &hash).expect("save"),
            SaveOutcome::Conflict { disk_hash: None }
        );
        assert!(!fx.vault.root().join("a.md").exists(), "not recreated");
    }

    #[test]
    fn saving_refuses_paths_outside_the_vault_and_files_that_are_not_markdown() {
        let fx = Fx::new(&[("a.md", b"x"), ("n.txt", b"x")]);
        let ed = fx.editor();
        let h = hash_hex(b"x");
        assert_eq!(kind_of(ed.save("../x.md", "y", &h)), "pathEscape");
        assert_eq!(kind_of(ed.save("C:/Windows/x.md", "y", &h)), "invalidPath");
        assert_eq!(kind_of(ed.save("n.txt", "y", &h)), "notMarkdown");
        assert_eq!(fx.read("n.txt"), b"x");
    }

    #[test]
    fn an_editor_that_takes_text_files_reads_and_saves_them_and_still_refuses_other_kinds() {
        let fx = Fx::new(&[
            ("n.txt", b"plain\r\ntext\r\n"),
            ("d.json", b"{}"),
            ("a.md", b"x"),
        ]);
        let ed = fx.editor().with_text_files();
        let source = ed.source("n.txt").expect("text is edited");
        assert_eq!(source.text, "plain\r\ntext\r\n");
        let saved = ed.save("n.txt", "plain\r\nmore\r\n", &source.hash);
        assert!(matches!(saved, Ok(SaveOutcome::Saved { .. })));
        assert_eq!(fx.read("n.txt"), b"plain\r\nmore\r\n");
        assert_eq!(kind_of(ed.source("d.json")), "notMarkdown");
        assert!(ed.source("a.md").is_ok());
        // the plain editor of a project leaves text files alone
        assert_eq!(kind_of(fx.editor().source("n.txt")), "notMarkdown");
    }

    #[test]
    fn a_read_only_file_is_not_overwritten() {
        let fx = Fx::new(&[("a.md", b"x")]);
        let path = fx.vault.root().join("a.md");
        let original = fs::metadata(&path).expect("meta").permissions();
        let mut read_only = original.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only).expect("chmod");
        let result = fx.editor().save("a.md", "y", &hash_hex(b"x"));
        fs::set_permissions(&path, original).expect("chmod back");
        assert_eq!(kind_of(result), "denied");
        assert_eq!(fx.read("a.md"), b"x");
    }

    #[test]
    fn a_save_is_announced_to_the_watcher_and_a_refused_one_is_not() {
        let fx = Fx::new(&[("a.md", b"x")]);
        let ed = fx.editor();
        let hash = hash_hex(b"x");
        ed.save("a.md", "y\n", &hash).expect("save");
        let sigs = crate::watch::SigStore::new();
        sigs.set(
            crate::vault::key("a.md"),
            "a.md",
            crate::watch::FileSig {
                len: 2,
                mtime_ms: 1,
                hash: hash_bytes(b"y\n"),
            },
        );
        let mut changes = vec![crate::watch::FsChange {
            kind: crate::watch::ChangeKind::Modified,
            path: "a.md".to_owned(),
            from: None,
            at_ms: 1,
        }];
        fx.own.strip(&mut changes, &sigs);
        assert_eq!(changes, Vec::<crate::watch::FsChange>::new());

        // A conflict writes nothing and announces nothing.
        fs::write(fx.vault.root().join("a.md"), "agent").expect("write");
        assert!(matches!(
            ed.save("a.md", "mine", &hash),
            Ok(SaveOutcome::Conflict { .. })
        ));
        sigs.set(
            crate::vault::key("a.md"),
            "a.md",
            crate::watch::FileSig {
                len: 4,
                mtime_ms: 2,
                hash: hash_bytes(b"mine"),
            },
        );
        let mut changes = vec![crate::watch::FsChange {
            kind: crate::watch::ChangeKind::Modified,
            path: "a.md".to_owned(),
            from: None,
            at_ms: 2,
        }];
        fx.own.strip(&mut changes, &sigs);
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn creating_a_file_writes_it_once_and_never_overwrites() {
        let fx = Fx::new(&[("docs/old.md", b"keep")]);
        let ed = fx.editor();
        let made = ed
            .create("docs/New Note.md", "# New Note\n")
            .expect("create");
        assert_eq!(made.path, "docs/New Note.md");
        assert_eq!(made.hash, hash_hex(b"# New Note\n"));
        assert_eq!(fx.read("docs/New Note.md"), b"# New Note\n");

        assert_eq!(kind_of(ed.create("docs/New Note.md", "again")), "exists");
        assert_eq!(kind_of(ed.create("docs/old.md", "again")), "exists");
        assert_eq!(fx.read("docs/old.md"), b"keep");
        if crate::vault::CASE_INSENSITIVE {
            assert_eq!(kind_of(ed.create("docs/NEW NOTE.md", "x")), "exists");
        }
        assert_eq!(kind_of(ed.create("docs/x.txt", "x")), "notMarkdown");
        assert_eq!(kind_of(ed.create("../x.md", "x")), "pathEscape");
        assert_eq!(kind_of(ed.create("nodir/x.md", "x")), "notFound");
    }

    #[test]
    fn creating_a_file_checks_the_name() {
        let fx = Fx::new(&[]);
        let ed = fx.editor();
        for bad in [
            "a?.md",
            "a*.md",
            "a<b.md",
            "a|b.md",
            "a\"b.md",
            "con.md",
            "NUL.md",
            "con .md",
            "Lpt1  .md",
        ] {
            assert_eq!(kind_of(ed.create(bad, "x")), "invalidName", "{bad}");
        }
        // a bare extension is a hidden file, not a Markdown file
        assert_eq!(kind_of(ed.create(".md", "x")), "notMarkdown");
        assert!(fs::read_dir(fx.vault.root()).expect("ls").next().is_none());
        assert!(validate_file_name("meeting notes 2026.md").is_ok());
        assert!(validate_file_name("Résumé – draft.md").is_ok());
        assert!(validate_file_name("trailing.").is_err());
        assert!(validate_file_name("a\u{7}b.md").is_err());
    }

    #[test]
    fn a_created_file_is_announced_to_the_watcher() {
        let fx = Fx::new(&[]);
        fx.editor().create("n.md", "# N\n").expect("create");
        let sigs = crate::watch::SigStore::new();
        sigs.set(
            crate::vault::key("n.md"),
            "n.md",
            crate::watch::FileSig {
                len: 4,
                mtime_ms: 1,
                hash: hash_bytes(b"# N\n"),
            },
        );
        let mut changes = vec![crate::watch::FsChange {
            kind: crate::watch::ChangeKind::Created,
            path: "n.md".to_owned(),
            from: None,
            at_ms: 1,
        }];
        fx.own.strip(&mut changes, &sigs);
        assert_eq!(changes, Vec::<crate::watch::FsChange>::new());
    }

    #[test]
    fn fragments_are_rendered_like_the_reading_view_and_stay_in_order() {
        let html = render_fragments(&[
            "| a | b |
|---|---|
| 1 | 2 |"
                .to_owned(),
            "<div class=\"note\">raw <script>alert(1)</script></div>".to_owned(),
            "```mermaid
graph LR
  A --> B
```"
            .to_owned(),
            "Inline $x^2$ math.".to_owned(),
            String::new(),
        ]);
        assert_eq!(html.len(), 5);
        assert!(html[0].contains("<table"), "{}", html[0]);
        assert!(html[1].contains("raw"), "{}", html[1]);
        assert!(!html[1].contains("script"), "sanitized: {}", html[1]);
        assert!(html[2].contains("language-mermaid"), "{}", html[2]);
        assert!(html[3].contains("data-math-style"), "{}", html[3]);
        assert_eq!(html[4], "");
    }

    #[test]
    fn fragments_beyond_the_size_limit_come_back_empty() {
        let big = "a".repeat(usize::try_from(MAX_RENDER_BYTES).expect("fits") / 2 + 1);
        let html = render_fragments(&[big.clone(), big, "after".to_owned()]);
        assert!(html[0].contains('a'));
        assert_eq!(html[1], "", "the second piece does not fit");
        // the piece that did not fit takes nothing from what is left, so a small one after it still does
        assert!(html[2].contains("after"), "{}", html[2]);
    }

    #[test]
    fn errors_serialize_with_a_kind_the_ui_can_branch_on() {
        let kinds = [
            (EditError::Binary("a".into()), "binary"),
            (EditError::TooLarge("a".into()), "tooLarge"),
            (EditError::NotMarkdown("a".into()), "notMarkdown"),
            (EditError::Encoding("a".into()), "encoding"),
            (EditError::Exists("a".into()), "exists"),
            (EditError::InvalidName(NameProblem::Empty), "invalidName"),
            (EditError::Unsupported("a".into()), "unsupported"),
            (EditError::InvalidData("a".into()), "invalidData"),
            (
                EditError::Io {
                    path: "a".into(),
                    source: io::Error::from(io::ErrorKind::PermissionDenied),
                },
                "denied",
            ),
            (
                EditError::Io {
                    path: "a".into(),
                    source: io::Error::other("disk"),
                },
                "io",
            ),
        ];
        for (error, kind) in kinds {
            assert_eq!(error.kind(), kind);
        }
        let json = serde_json::to_value(SaveOutcome::Conflict { disk_hash: None }).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"result": "conflict", "diskHash": null})
        );
        let json = serde_json::to_value(SaveOutcome::Saved {
            hash: "ab".into(),
            modified_ms: 5,
        })
        .expect("json");
        assert_eq!(
            json,
            serde_json::json!({"result": "saved", "hash": "ab", "modifiedMs": 5})
        );
    }
}
