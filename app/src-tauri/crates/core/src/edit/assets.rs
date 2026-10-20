//! Pictures pasted | dropped into a document: allowed types, their signatures, writing a file that must be new.

use std::io::{self, Write};
use std::path::Path;

use crate::error::{ErrorKind, IpcError};

/// Hard limit of one pasted picture.
pub const MAX_ASSET_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Png,
    Jpg,
    Gif,
    Webp,
    Svg,
}

/// The picture type of an extension | MIME subtype (`png`, `jpeg`, `svg+xml`, ...).
#[must_use]
pub fn asset_kind(ext: &str) -> Option<AssetKind> {
    match ext
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some(AssetKind::Png),
        "jpg" | "jpeg" => Some(AssetKind::Jpg),
        "gif" => Some(AssetKind::Gif),
        "webp" => Some(AssetKind::Webp),
        "svg" | "svg+xml" => Some(AssetKind::Svg),
        _ => None,
    }
}

impl AssetKind {
    #[must_use]
    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }

    /// Whether the bytes are of this type (signature; SVG: text with an `<svg` element near the start).
    #[must_use]
    pub fn matches(self, bytes: &[u8]) -> bool {
        match self {
            Self::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpg => bytes.starts_with(b"\xFF\xD8\xFF"),
            Self::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            Self::Webp => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
            Self::Svg => {
                let head = &bytes[..bytes.len().min(4096)];
                std::str::from_utf8(head)
                    .or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()]))
                    .is_ok_and(|text| !text.contains('\0') && text.contains("<svg"))
            }
        }
    }
}

/// The bytes of a picture file dropped from the system onto an editor (the drop event carries paths, not files).
///
/// # Errors
/// Not an absolute path | not a picture type | not a regular file | bytes not of that type (`unsupported`); over the
/// limit (`tooLarge`); IO.
pub fn read_picture(path: &Path) -> Result<Vec<u8>, IpcError> {
    let shown = path.display().to_string();
    let kind = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(asset_kind)
        .filter(|_| path.is_absolute())
        .ok_or_else(|| IpcError::new(ErrorKind::Unsupported).subject(shown.clone()))?;
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(IpcError::new(ErrorKind::NotFound).subject(shown));
    }
    let bytes = crate::fsx::read_bytes(path, MAX_ASSET_BYTES)?;
    if !kind.matches(&bytes) {
        return Err(IpcError::new(ErrorKind::Unsupported)
            .subject(shown)
            .detail("content"));
    }
    Ok(bytes)
}

/// Creates `abs` (must not exist) with `bytes`, durably; a failed write leaves nothing behind.
///
/// # Errors
/// `AlreadyExists`, IO.
pub(super) fn write_new(abs: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = abs.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(abs)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = std::fs::remove_file(abs);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG signature and four more bytes.
    const PNG_HEAD: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    #[test]
    fn a_dropped_picture_is_read_when_it_is_what_its_name_says() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("p.PNG");
        std::fs::write(&png, PNG_HEAD).unwrap();
        assert_eq!(read_picture(&png).unwrap().len(), 12);
        // a name that lies, a type that is no picture, a folder, a missing file, a relative path
        let lie = dir.path().join("lie.png");
        std::fs::write(&lie, b"not a png").unwrap();
        let text = dir.path().join("t.txt");
        std::fs::write(&text, PNG_HEAD).unwrap();
        for bad in [&lie, &text, dir.path(), &dir.path().join("none.png")] {
            assert!(read_picture(bad).is_err(), "{}", bad.display());
        }
        assert!(read_picture(Path::new("p.png")).is_err());
        let folder = dir.path().join("dir.png");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(read_picture(&folder).unwrap_err().kind, ErrorKind::NotFound);
    }

    #[test]
    fn kinds_by_extension_and_mime() {
        assert_eq!(asset_kind("PNG"), Some(AssetKind::Png));
        assert_eq!(asset_kind(".jpeg"), Some(AssetKind::Jpg));
        assert_eq!(asset_kind("svg+xml"), Some(AssetKind::Svg));
        assert_eq!(asset_kind("exe"), None);
        assert_eq!(asset_kind(""), None);
    }

    #[test]
    fn signatures_must_match() {
        assert!(AssetKind::Png.matches(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!AssetKind::Png.matches(b"GIF89a"));
        assert!(AssetKind::Gif.matches(b"GIF89a..."));
        assert!(AssetKind::Jpg.matches(b"\xFF\xD8\xFF\xE0"));
        assert!(AssetKind::Webp.matches(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!AssetKind::Webp.matches(b"RIFF"));
        assert!(AssetKind::Svg.matches(b"<?xml version=\"1.0\"?><svg xmlns=\"x\"/>"));
        assert!(!AssetKind::Svg.matches(b"MZ\x90\0 not a picture"));
        assert!(!AssetKind::Svg.matches(b""));
    }
}
