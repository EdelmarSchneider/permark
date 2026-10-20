//! Large text: the start of a file over the size limit, and the chunks after it, so that a very large file shows its
//! first part at once and the rest as the reader comes to it. Nothing here reads a whole file.
//!
//! A chunk ends at a line break (the last one in what was read); a line longer than a chunk is cut at a character
//! boundary and the next chunk goes on with it (the viewer joins them: it holds text that does not end with a break).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::fsx::{self, FsxError};

/// What the first view of a large file holds.
pub const FIRST_CHUNK: usize = 1024 * 1024;
/// What each of the following chunks holds.
pub const MORE_CHUNK: usize = 2 * 1024 * 1024;

/// How much of a file a view holds (bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Partial {
    /// Where the next chunk starts.
    #[ts(type = "number")]
    pub loaded: u64,
    /// The size of the file.
    #[ts(type = "number")]
    pub total: u64,
}

/// A chunk of a large file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TextChunk {
    pub text: String,
    /// Where the chunk after this one starts (`None`: this was the end).
    #[ts(type = "number | null")]
    pub next: Option<u64>,
}

/// What the start of a large file is.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Start {
    Text(TextChunk, Partial),
    /// NUL bytes in the head.
    Binary,
    /// UTF-16 (a byte-order mark): not cut into chunks.
    Utf16,
}

/// Reads up to `len` bytes of the file from `offset`.
fn read_at(abs: &Path, offset: u64, len: usize) -> Result<Vec<u8>, FsxError> {
    let shown = || abs.display().to_string();
    let io = |source| FsxError::Io {
        path: shown(),
        source,
    };
    let mut file = std::fs::File::open(abs).map_err(io)?;
    file.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut bytes = Vec::with_capacity(len);
    file.take(len as u64).read_to_end(&mut bytes).map_err(io)?;
    Ok(bytes)
}

/// How many of `bytes` a chunk takes: up to the last line break; when there is none, up to a character boundary.
/// `end`: the bytes reach the end of the file, so all of them.
#[must_use]
pub(super) fn cut(bytes: &[u8], end: bool) -> usize {
    if end {
        return bytes.len();
    }
    if let Some(at) = bytes.iter().rposition(|&b| b == b'\n') {
        return at + 1;
    }
    // a line longer than the chunk: not in the middle of a character
    let mut taken = bytes.len();
    let floor = bytes.len().saturating_sub(4);
    while taken > floor && bytes[taken - 1] & 0xC0 == 0x80 {
        taken -= 1;
    }
    if taken > 0 && bytes[taken - 1] >= 0xC0 {
        taken -= 1;
    }
    if taken == 0 {
        bytes.len()
    } else {
        taken
    }
}

/// The chunk `bytes` (read at `offset` of a file of `size`) make, and where the next one starts.
fn chunk_of(bytes: &[u8], offset: u64, size: u64) -> (TextChunk, u64) {
    let taken = cut(bytes, offset + bytes.len() as u64 >= size);
    let next = offset + taken as u64;
    let text = String::from_utf8_lossy(&bytes[..taken]).into_owned();
    (
        TextChunk {
            text,
            next: (next < size).then_some(next),
        },
        next,
    )
}

/// The start of a file larger than the limit.
pub(super) fn first(abs: &Path, size: u64) -> Result<Start, FsxError> {
    let bytes = read_at(abs, 0, FIRST_CHUNK)?;
    if bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF") {
        return Ok(Start::Utf16);
    }
    if bytes[..bytes.len().min(fsx::BINARY_SNIFF)].contains(&0) {
        return Ok(Start::Binary);
    }
    // a byte-order mark is not text
    let skip = if bytes.starts_with(b"\xEF\xBB\xBF") {
        3
    } else {
        0
    };
    let (chunk, next) = chunk_of(&bytes[skip..], skip as u64, size);
    Ok(Start::Text(
        chunk,
        Partial {
            loaded: next,
            total: size,
        },
    ))
}

/// The chunk of a file from `offset`; `size` and `mtime_ms` are the ones the viewer opened the file with.
pub(super) fn more(
    abs: &Path,
    path: &str,
    offset: u64,
    size: u64,
    mtime_ms: u64,
) -> Result<TextChunk, IpcError> {
    let meta = std::fs::metadata(abs).map_err(|e| IpcError::io(path, &e))?;
    if !meta.is_file() {
        return Err(IpcError::new(ErrorKind::NotFound).subject(path));
    }
    if meta.len() != size || super::mtime_ms(&meta) != mtime_ms {
        return Err(IpcError::new(ErrorKind::Conflict)
            .subject(path)
            .detail("changed"));
    }
    if offset >= size {
        return Ok(TextChunk {
            text: String::new(),
            next: None,
        });
    }
    let bytes = read_at(abs, offset, MORE_CHUNK)?;
    Ok(chunk_of(&bytes, offset, size).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunk_ends_at_the_last_line_break() {
        assert_eq!(cut(b"one\ntwo\nthr", false), 8);
        assert_eq!(cut(b"one\ntwo\n", false), 8);
        // the end of the file: everything
        assert_eq!(cut(b"one\ntwo\nthr", true), 11);
        assert_eq!(cut(b"", true), 0);
    }

    #[test]
    fn a_line_longer_than_a_chunk_is_not_cut_inside_a_character() {
        let line = "é".repeat(10); // two bytes each
        let bytes = line.as_bytes();
        // cut in the middle of a character: the half goes to the next chunk
        let taken = cut(&bytes[..bytes.len() - 1], false);
        assert_eq!(taken, bytes.len() - 2);
        assert!(std::str::from_utf8(&bytes[..taken]).is_ok());
        // a four-byte character
        let emoji = "😀😀😀".as_bytes();
        for cut_at in 4..emoji.len() {
            let taken = cut(&emoji[..cut_at], false);
            assert!(std::str::from_utf8(&emoji[..taken]).is_ok(), "at {cut_at}");
            assert!(taken > 0);
        }
        // nothing but one half character: taken whole (there is nowhere else to cut)
        assert_eq!(cut(&emoji[..2], false), 2);
    }
}
