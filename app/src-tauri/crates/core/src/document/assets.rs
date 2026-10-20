//! What the asset protocol serves: pictures, sound and video of the open workspace, whole or by byte ranges.
//! The caller resolves the workspace and the path through the vault (the boundary); this module decides what the
//! file may be served as and which bytes go out.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::format::{media_of, MediaKind};

/// The most that one answer carries of a sound or a video. A player asks for `bytes=0-` and for the rest as it plays,
/// so the answer may be shorter than asked: the player asks again, and a long file is never held whole in memory.
pub const MAX_CHUNK: u64 = 4 * 1024 * 1024;

/// A picture, or a sound | video without a `Range` header, is answered whole up to this size.
pub const MAX_WHOLE: u64 = 32 * 1024 * 1024;

/// Which bytes of a file an answer carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Whole,
    /// Bytes `start` to `end`, both included.
    Part {
        start: u64,
        end: u64,
    },
    /// The request starts after the end of the file.
    Unsatisfiable,
}

/// The bytes a `Range` header asks for in a file of `len` bytes, at most `chunk` of them. Only the first range of the
/// header is served. A header that cannot be read, or is not in bytes, is ignored as the HTTP rules say: the whole
/// file is answered.
#[must_use]
pub fn window(header: Option<&str>, len: u64, chunk: u64) -> Window {
    let Some(header) = header else {
        return Window::Whole;
    };
    let Some((unit, specs)) = header.split_once('=') else {
        return Window::Whole;
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") || len == 0 {
        return Window::Whole;
    }
    let spec = specs.split(',').next().unwrap_or_default().trim();
    let Some((first, last)) = spec.split_once('-') else {
        return Window::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    let number = |text: &str| text.parse::<u64>().ok();
    let (start, end) = match (first, last) {
        // `-500`: the last 500 bytes
        ("", suffix) => match number(suffix) {
            Some(0) => return Window::Unsatisfiable,
            Some(n) => (len.saturating_sub(n), len - 1),
            None => return Window::Whole,
        },
        // `500-`: from byte 500 on
        (from, "") => match number(from) {
            Some(start) => (start, len - 1),
            None => return Window::Whole,
        },
        (from, to) => match (number(from), number(to)) {
            (Some(start), Some(end)) if start <= end => (start, end.min(len - 1)),
            _ => return Window::Whole,
        },
    };
    if start >= len {
        return Window::Unsatisfiable;
    }
    let end = end.min(start.saturating_add(chunk.max(1) - 1));
    Window::Part { start, end }
}

/// What the answer carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Whole,
    Range { start: u64, end: u64 },
    Unsatisfiable,
}

/// A file answer: bytes + the facts for the headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub part: Part,
    /// Size of the whole file.
    pub total: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AssetError {
    /// Not a picture | sound | video by name, or the file behind a link is another kind of file.
    #[error("not a picture, sound or video")]
    NotMedia,
    #[error("not found")]
    NotFound,
    #[error("cannot read the file: {0}")]
    Io(String),
}

/// What a file is served as, when a request for `requested` was resolved (links followed) to the file `resolved`:
/// the kind and MIME type from the name the file really has. A link named `a.png` that points at `.env` is not a
/// picture, so nothing is served through it; a link between two pictures is served as the picture it leads to.
#[must_use]
pub fn served_as(requested: &str, resolved: &Path) -> Option<(MediaKind, &'static str)> {
    media_of(requested)?;
    media_of(&resolved.file_name()?.to_string_lossy())
}

fn read_part(path: &Path, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    crate::fsx::retry(&crate::fsx::RETRY_DELAYS, || {
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(end - start + 1).read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

/// Reads the answer for `requested` (the name asked for) found at `resolved` (the absolute file).
///
/// # Errors
/// `NotMedia`, `NotFound` (also for a picture over [`MAX_WHOLE`]), `Io`.
pub fn read(requested: &str, resolved: &Path, range: Option<&str>) -> Result<Asset, AssetError> {
    let (kind, mime) = served_as(requested, resolved).ok_or(AssetError::NotMedia)?;
    let meta = std::fs::metadata(resolved).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AssetError::NotFound,
        _ => AssetError::Io(e.to_string()),
    })?;
    if !meta.is_file() {
        return Err(AssetError::NotFound);
    }
    let total = meta.len();
    let streamed = matches!(kind, MediaKind::Audio | MediaKind::Video);
    let wanted = if streamed {
        match window(range, total, MAX_CHUNK) {
            // no range asked for: a big file starts with its first part, a small one is sent whole
            Window::Whole if total > MAX_WHOLE => window(Some("bytes=0-"), total, MAX_CHUNK),
            other => other,
        }
    } else if total > MAX_WHOLE {
        return Err(AssetError::NotFound);
    } else {
        Window::Whole
    };
    let io = |e: std::io::Error| AssetError::Io(e.to_string());
    let (bytes, part) = match wanted {
        Window::Whole => {
            // at most what the file measured above, which the limit was checked on
            let bytes = match total.checked_sub(1) {
                Some(last) => read_part(resolved, 0, last).map_err(io)?,
                None => Vec::new(),
            };
            (bytes, Part::Whole)
        }
        Window::Part { start, end } => {
            let bytes = read_part(resolved, start, end).map_err(io)?;
            // the file got shorter than the answer was planned for: say what is really carried
            match u64::try_from(bytes.len()) {
                Ok(len) if len > 0 => (
                    bytes,
                    Part::Range {
                        start,
                        end: start + len - 1,
                    },
                ),
                _ => (Vec::new(), Part::Unsatisfiable),
            }
        }
        Window::Unsatisfiable => (Vec::new(), Part::Unsatisfiable),
    };
    Ok(Asset {
        mime,
        bytes,
        part,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_windows() {
        assert_eq!(window(None, 100, 10), Window::Whole);
        assert_eq!(
            window(Some("bytes=0-9"), 100, 50),
            Window::Part { start: 0, end: 9 }
        );
        assert_eq!(
            window(Some("bytes=10-"), 100, 50),
            Window::Part { start: 10, end: 59 }
        );
        assert_eq!(
            window(Some("bytes=-5"), 100, 50),
            Window::Part { start: 95, end: 99 }
        );
        assert_eq!(
            window(Some("bytes=90-500"), 100, 50),
            Window::Part { start: 90, end: 99 }
        );
        assert_eq!(
            window(Some("bytes=0-0,5-9"), 100, 50),
            Window::Part { start: 0, end: 0 }
        );
        assert_eq!(
            window(Some(" BYTES = 2 - 3 "), 100, 50),
            Window::Part { start: 2, end: 3 }
        );
        // past the end, empty suffix
        assert_eq!(window(Some("bytes=100-"), 100, 50), Window::Unsatisfiable);
        assert_eq!(
            window(Some("bytes=500-600"), 100, 50),
            Window::Unsatisfiable
        );
        assert_eq!(window(Some("bytes=-0"), 100, 50), Window::Unsatisfiable);
        // the whole file for what cannot be read
        for header in [
            "",
            "bytes",
            "bytes=",
            "bytes=a-b",
            "bytes=5-2",
            "items=0-5",
            "bytes=--5",
            "bytes=1-2-3",
            "bytes=-",
        ] {
            assert_eq!(window(Some(header), 100, 50), Window::Whole, "{header}");
        }
        assert_eq!(window(Some("bytes=0-9"), 0, 50), Window::Whole);
        // a suffix longer than the file is the whole file as one part; huge numbers never overflow
        assert_eq!(
            window(Some("bytes=-500"), 100, 1000),
            Window::Part { start: 0, end: 99 }
        );
        assert_eq!(
            window(Some("bytes=18446744073709551615-"), 100, 50),
            Window::Unsatisfiable
        );
        assert_eq!(
            window(Some("bytes=0-18446744073709551615"), u64::MAX, u64::MAX),
            Window::Part {
                start: 0,
                end: u64::MAX - 1
            }
        );
    }

    #[test]
    fn serving_needs_media_names_on_both_sides() {
        assert!(served_as("img/a.png", Path::new("/p/img/a.png")).is_some());
        assert!(served_as("a.png", Path::new("/p/b.jpg")).is_some());
        assert_eq!(served_as("a.png", Path::new("/p/.env")), None);
        assert_eq!(served_as("a.png", Path::new("/p/secret.txt")), None);
        assert_eq!(served_as("notes.md", Path::new("/p/notes.md")), None);
        assert_eq!(served_as("a.png", Path::new("/")), None);
    }

    #[test]
    fn reads_whole_ranges_and_refuses_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("song.mp3");
        std::fs::write(&song, (0..=255u8).cycle().take(1000).collect::<Vec<_>>()).unwrap();
        let pic = dir.path().join("a b.png");
        std::fs::write(&pic, b"PNGDATA").unwrap();
        std::fs::write(dir.path().join("empty.png"), b"").unwrap();
        std::fs::write(dir.path().join("note.txt"), b"x").unwrap();

        let whole = read("a b.png", &pic, Some("bytes=0-1")).unwrap();
        assert_eq!(
            (whole.part, whole.bytes.len(), whole.mime),
            (Part::Whole, 7, "image/png")
        );
        let part = read("song.mp3", &song, Some("bytes=10-19")).unwrap();
        assert_eq!(part.part, Part::Range { start: 10, end: 19 });
        assert_eq!(part.bytes, (10..20u8).collect::<Vec<_>>());
        assert_eq!(part.total, 1000);
        assert_eq!(read("song.mp3", &song, None).unwrap().part, Part::Whole);
        assert_eq!(
            read("song.mp3", &song, Some("bytes=5000-")).unwrap().part,
            Part::Unsatisfiable
        );
        assert_eq!(
            read("empty.png", &dir.path().join("empty.png"), None)
                .unwrap()
                .bytes
                .len(),
            0
        );
        // not media by either name
        assert_eq!(
            read("note.txt", &dir.path().join("note.txt"), None),
            Err(AssetError::NotMedia)
        );
        assert_eq!(
            read("a.png", &dir.path().join("note.txt"), None),
            Err(AssetError::NotMedia)
        );
        assert_eq!(
            read("gone.png", &dir.path().join("gone.png"), None),
            Err(AssetError::NotFound)
        );
        // a folder named like a picture
        std::fs::create_dir(dir.path().join("dir.png")).unwrap();
        assert_eq!(
            read("dir.png", &dir.path().join("dir.png"), None),
            Err(AssetError::NotFound)
        );
    }
}
