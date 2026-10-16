//! The pictures, sound and video of a folder: which files are which, how a part of a big file is asked for and cut out
//! of it, and which files may be handed to the system's default application.
//!
//! Nothing here touches a window or a folder's boundaries: a path is checked against the folder first (see
//! [`crate::vault::Vault::resolve`]) and only then reaches these functions.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use crate::util::retry_read;

/// What a file is shown as, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Audio,
    Video,
}

impl AssetKind {
    /// Audio and video are cut into parts as they are played; a picture is read whole.
    pub fn is_streamed(self) -> bool {
        matches!(self, Self::Audio | Self::Video)
    }
}

/// Every extension the app shows, its kind and its MIME type. The webview plays what its codecs allow: a file it
/// cannot play (`mkv` with an unusual codec, `avi`, `wmv`) is still listed here, so that it is found, opened by name
/// and handed to the system's application with a clear message.
const ASSETS: &[(&str, AssetKind, &str)] = &[
    ("png", AssetKind::Image, "image/png"),
    ("jpg", AssetKind::Image, "image/jpeg"),
    ("jpeg", AssetKind::Image, "image/jpeg"),
    ("gif", AssetKind::Image, "image/gif"),
    ("webp", AssetKind::Image, "image/webp"),
    ("avif", AssetKind::Image, "image/avif"),
    ("bmp", AssetKind::Image, "image/bmp"),
    ("ico", AssetKind::Image, "image/x-icon"),
    ("svg", AssetKind::Image, "image/svg+xml"),
    ("mp3", AssetKind::Audio, "audio/mpeg"),
    ("wav", AssetKind::Audio, "audio/wav"),
    ("ogg", AssetKind::Audio, "audio/ogg"),
    ("oga", AssetKind::Audio, "audio/ogg"),
    ("opus", AssetKind::Audio, "audio/ogg"),
    ("flac", AssetKind::Audio, "audio/flac"),
    ("m4a", AssetKind::Audio, "audio/mp4"),
    ("aac", AssetKind::Audio, "audio/aac"),
    ("weba", AssetKind::Audio, "audio/webm"),
    ("wma", AssetKind::Audio, "audio/x-ms-wma"),
    ("aif", AssetKind::Audio, "audio/aiff"),
    ("aiff", AssetKind::Audio, "audio/aiff"),
    ("mka", AssetKind::Audio, "audio/x-matroska"),
    ("mp4", AssetKind::Video, "video/mp4"),
    ("m4v", AssetKind::Video, "video/mp4"),
    ("webm", AssetKind::Video, "video/webm"),
    ("ogv", AssetKind::Video, "video/ogg"),
    ("mov", AssetKind::Video, "video/quicktime"),
    ("mkv", AssetKind::Video, "video/x-matroska"),
    ("avi", AssetKind::Video, "video/x-msvideo"),
    ("wmv", AssetKind::Video, "video/x-ms-wmv"),
    ("flv", AssetKind::Video, "video/x-flv"),
    ("mpg", AssetKind::Video, "video/mpeg"),
    ("mpeg", AssetKind::Video, "video/mpeg"),
    ("3gp", AssetKind::Video, "video/3gpp"),
];

/// The lower-case extension of a path, once the dots and spaces that end a name are dropped: Windows ignores them, so
/// `evil.exe.` is `evil.exe` there.
fn extension_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = name.trim_end_matches(['.', ' ']);
    let (stem, ext) = name.rsplit_once('.')?;
    // `.gitignore` is a name, not an extension
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

fn lookup(path: &str) -> Option<(AssetKind, &'static str)> {
    let ext = extension_of(path)?;
    ASSETS
        .iter()
        .find(|(known, _, _)| *known == ext)
        .map(|(_, kind, mime)| (*kind, *mime))
}

/// What kind of picture, sound or video the path names, if any.
pub fn asset_kind(path: &str) -> Option<AssetKind> {
    lookup(path).map(|(kind, _)| kind)
}

/// The MIME type of a picture, sound or video, from its extension.
pub fn asset_mime(path: &str) -> Option<&'static str> {
    lookup(path).map(|(_, mime)| mime)
}

// ---- parts of a file -------------------------------------------------------------------------------------

/// The most that one answer carries of a sound or a video. A player asks for `bytes=0-` and for the rest as it plays,
/// so the answer may be shorter than asked: the player asks again, and a long file is never held whole in memory.
pub const MAX_CHUNK: u64 = 4 * 1024 * 1024;

/// A sound or video without a `Range` header (a download, a very old player) is answered whole up to this size;
/// a bigger one is answered with its first part.
pub const MAX_WHOLE: u64 = 32 * 1024 * 1024;

/// Which bytes of a file an answer carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// The whole file.
    Whole,
    /// The bytes `start` to `end`, both included.
    Part { start: u64, end: u64 },
    /// The request starts after the end of the file.
    Unsatisfiable,
}

/// The bytes a `Range` header asks for in a file of `len` bytes, at most `chunk` of them. Only the first range of the
/// header is served. A header that cannot be read, or that is not in bytes, is ignored as the HTTP rules say: the whole
/// file is answered.
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

/// The bytes `start` to `end` (both included) of the file, as many as it still has when it shrank meanwhile.
pub fn read_part(path: &Path, start: u64, end: u64) -> io::Result<Vec<u8>> {
    retry_read(|| {
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(end - start + 1).read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

// ---- what may be opened by the system --------------------------------------------------------------------

/// Extensions of files that the system shows and never starts: documents, PDF files, office files, archives and the
/// like. Together with the pictures, sound and video, they are exempt from the execute-permission check of
/// [`open_policy`] (a Windows, exFAT or network drive often gives every file that permission).
const OPENABLE_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdown", "mkd", "mdx", "txt", "text", "log", "rst", "adoc", "org", "tex",
    "bib", "json", "jsonc", "yaml", "yml", "toml", "xml", "xsd", "csv", "tsv", "ini", "cfg",
    "conf", "html", "htm", "css", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods",
    "odp", "rtf", "epub", "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz", "psd", "ai",
];

/// Whether the extension is one of a document, a picture, sound or video: files the system shows and never starts.
pub fn is_openable_extension(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    OPENABLE_EXTENSIONS.contains(&ext.as_str()) || ASSETS.iter().any(|(known, _, _)| *known == ext)
}

/// Extensions the system starts as a program, a script, an installer, a shortcut or a macro document when it opens them.
/// Opening one of these from a click would run whatever an agent or a repository put in the folder.
const PROGRAM_EXTENSIONS: &[&str] = &[
    // Windows programs, installers and shortcuts
    "exe",
    "com",
    "scr",
    "pif",
    "msi",
    "msp",
    "mst",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "application",
    "appref-ms",
    "gadget",
    "lnk",
    "url",
    "scf",
    "inf",
    "dll",
    "sys",
    "drv",
    "ocx",
    "cpl",
    "msc",
    "jar",
    "jnlp",
    "chm",
    "diagcab",
    "library-ms",
    "search-ms",
    "settingcontent-ms",
    // Windows scripts
    "bat",
    "cmd",
    "btm",
    "ps1",
    "ps1xml",
    "ps2",
    "ps2xml",
    "psc1",
    "psc2",
    "psd1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "ws",
    "wsc",
    "sct",
    "hta",
    "reg",
    // Python, Perl and Ruby scripts, which the Windows launchers run
    "py",
    "pyw",
    "pyc",
    "pyz",
    "pl",
    "rb",
    // Office documents with macros
    "docm",
    "dotm",
    "xlsm",
    "xltm",
    "xlam",
    "xll",
    "wll",
    "pptm",
    "potm",
    "ppam",
    "ppsm",
    "sldm",
    // macOS and Linux programs, scripts and installers
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "fish",
    "command",
    "tool",
    "run",
    "app",
    "pkg",
    "mpkg",
    "dmg",
    "deb",
    "rpm",
    "appimage",
    "desktop",
    "scpt",
    "scptd",
    "applescript",
    "workflow",
    "action",
    "osx",
    "out",
    "elf",
    "kext",
    "prefpane",
    "saver",
    "terminal",
    "webloc",
];

/// Whether a file with an execute permission is a program on this system: macOS and Linux run such a file (macOS in a
/// terminal) when the system "opens" it. Windows decides by the extension alone.
#[cfg(unix)]
pub fn has_execute_bit(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub fn has_execute_bit(_path: &Path) -> bool {
    false
}

/// Whether the system's default application may be asked to open a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPolicy {
    Allowed,
    /// A program, script, installer, shortcut or macro document, which is never opened from a click. `label` names the
    /// kind of file for the message: `.ps1`, or the file's name for a program without a known extension.
    Blocked {
        label: String,
    },
}

/// Why a file is not handed to the system's default application.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// A program, script, installer, shortcut or macro document. `label` names the kind of file (`.ps1`).
    #[error("{label} files are programs or scripts and are not opened from here")]
    Blocked { label: String },
}

/// The refusal for a file, or `Ok` when the system may open it (see [`open_policy`]).
pub fn check_openable(path: &Path) -> Result<(), OpenError> {
    match open_policy(path) {
        OpenPolicy::Allowed => Ok(()),
        OpenPolicy::Blocked { label } => Err(OpenError::Blocked { label }),
    }
}

/// What may be done with a file that is about to be opened by the system.
///
/// The extension decides, on the real file (a link is followed first, so a harmless name that points at a program
/// is judged by the program). On macOS and Linux a file with an execute permission is a program too, unless it is a
/// document, picture, sound or video (files on a Windows or network drive often have the permission on all of them).
pub fn open_policy(path: &Path) -> OpenPolicy {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = extension_of(&name);
    if let Some(ext) = &ext {
        if PROGRAM_EXTENSIONS.contains(&ext.as_str()) {
            return OpenPolicy::Blocked {
                label: format!(".{ext}"),
            };
        }
    }
    let known_safe = ext.as_deref().is_some_and(is_openable_extension);
    if !known_safe && has_execute_bit(path) {
        return OpenPolicy::Blocked { label: name };
    }
    OpenPolicy::Allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_names_the_kind_whatever_its_case() {
        for (path, kind) in [
            ("a.png", AssetKind::Image),
            ("dir/Logo.SVG", AssetKind::Image),
            ("photo.JPEG", AssetKind::Image),
            ("song.flac", AssetKind::Audio),
            ("voice.OPUS", AssetKind::Audio),
            ("clip.mp4", AssetKind::Video),
            ("dir\\movie.MKV", AssetKind::Video),
        ] {
            assert_eq!(asset_kind(path), Some(kind), "{path}");
        }
        for path in [
            "a.md",
            "png",
            ".png",
            "a.",
            "noext",
            "a.png.txt",
            "x.exe",
            "",
        ] {
            assert_eq!(asset_kind(path), None, "{path}");
        }
    }

    #[test]
    fn every_listed_format_has_a_mime_of_its_family() {
        for (ext, kind, mime) in ASSETS {
            let family = match kind {
                AssetKind::Image => "image/",
                AssetKind::Audio => "audio/",
                AssetKind::Video => "video/",
            };
            assert!(mime.starts_with(family), "{ext}: {mime}");
            assert_eq!(asset_mime(&format!("f.{ext}")), Some(*mime));
        }
        assert_eq!(asset_mime("a.mp4"), Some("video/mp4"));
        assert_eq!(asset_mime("a.svg"), Some("image/svg+xml"));
        assert_eq!(asset_mime("a.txt"), None);
    }

    #[test]
    fn a_name_ended_by_dots_or_spaces_is_judged_without_them() {
        // Windows drops them: `clip.mp4.` is `clip.mp4`, and `evil.exe ` is `evil.exe`
        assert_eq!(asset_kind("clip.mp4."), Some(AssetKind::Video));
        assert_eq!(asset_kind("clip.mp4 . "), Some(AssetKind::Video));
        assert_eq!(extension_of("evil.exe. "), Some("exe".to_owned()));
    }

    // ---- ranges ----

    const FIVE_MIB: u64 = 5 * 1024 * 1024;

    fn part(header: &str, len: u64) -> Window {
        window(Some(header), len, MAX_CHUNK)
    }

    #[test]
    fn no_header_means_the_whole_file() {
        assert_eq!(window(None, 1000, MAX_CHUNK), Window::Whole);
    }

    #[test]
    fn a_range_is_cut_out_of_the_file() {
        assert_eq!(part("bytes=0-99", 1000), Window::Part { start: 0, end: 99 });
        assert_eq!(
            part("bytes=500-", 1000),
            Window::Part {
                start: 500,
                end: 999
            }
        );
        assert_eq!(
            part("bytes=-100", 1000),
            Window::Part {
                start: 900,
                end: 999
            }
        );
        assert_eq!(
            part("Bytes = 10 - 20", 1000),
            Window::Part { start: 10, end: 20 }
        );
        // an end past the file is the end of the file
        assert_eq!(
            part("bytes=990-5000", 1000),
            Window::Part {
                start: 990,
                end: 999
            }
        );
        // a suffix longer than the file is the whole file
        assert_eq!(
            part("bytes=-5000", 1000),
            Window::Part { start: 0, end: 999 }
        );
    }

    #[test]
    fn only_the_first_of_several_ranges_is_served() {
        assert_eq!(
            part("bytes=0-9, 20-29", 1000),
            Window::Part { start: 0, end: 9 }
        );
    }

    #[test]
    fn an_answer_never_carries_more_than_one_chunk() {
        assert_eq!(
            part("bytes=0-", FIVE_MIB * 10),
            Window::Part {
                start: 0,
                end: MAX_CHUNK - 1
            }
        );
        assert_eq!(
            window(Some("bytes=100-"), 1000, 10),
            Window::Part {
                start: 100,
                end: 109
            }
        );
        // a suffix of a big file is the start of that suffix, and the player asks for the rest
        assert_eq!(
            part("bytes=-10485760", FIVE_MIB * 10),
            Window::Part {
                start: FIVE_MIB * 10 - 10 * 1024 * 1024,
                end: FIVE_MIB * 10 - 10 * 1024 * 1024 + MAX_CHUNK - 1
            }
        );
    }

    #[test]
    fn a_start_past_the_end_cannot_be_served() {
        assert_eq!(part("bytes=1000-", 1000), Window::Unsatisfiable);
        assert_eq!(part("bytes=2000-3000", 1000), Window::Unsatisfiable);
        assert_eq!(part("bytes=-0", 1000), Window::Unsatisfiable);
    }

    #[test]
    fn a_header_that_makes_no_sense_is_ignored() {
        for header in [
            "",
            "bytes",
            "bytes=",
            "bytes=abc-def",
            "bytes=10",
            "bytes=20-10",
            "items=0-5",
            "bytes=--5",
            "bytes=-",
            "bytes=18446744073709551616-",
        ] {
            assert_eq!(part(header, 1000), Window::Whole, "{header:?}");
        }
        // an empty file has no part to ask for
        assert_eq!(part("bytes=0-", 0), Window::Whole);
    }

    #[test]
    fn the_largest_offsets_do_not_overflow() {
        assert_eq!(
            window(Some("bytes=0-18446744073709551615"), u64::MAX, u64::MAX),
            Window::Part {
                start: 0,
                end: u64::MAX - 1
            }
        );
    }

    #[test]
    fn a_part_is_read_from_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.bin");
        std::fs::write(&file, b"0123456789").expect("write");
        assert_eq!(read_part(&file, 2, 5).expect("part"), b"2345");
        assert_eq!(read_part(&file, 0, 9).expect("all"), b"0123456789");
        // the file got shorter than the answer was planned for
        assert_eq!(read_part(&file, 8, 20).expect("short"), b"89");
        assert!(
            read_part(&file, 20, 30).expect("beyond").is_empty(),
            "nothing is left of the file past its end"
        );
        assert!(read_part(&dir.path().join("none.bin"), 0, 1).is_err());
    }

    // ---- opening with the system ----

    fn blocked(name: &str) -> Option<String> {
        match open_policy(Path::new(name)) {
            OpenPolicy::Blocked { label } => Some(label),
            OpenPolicy::Allowed => None,
        }
    }

    #[test]
    fn programs_and_scripts_are_never_opened() {
        for name in [
            "setup.exe",
            "run.bat",
            "run.CMD",
            "x.com",
            "tool.ps1",
            "a.vbs",
            "a.vbe",
            "a.js",
            "a.jse",
            "a.wsf",
            "a.scr",
            "link.lnk",
            "web.url",
            "patch.reg",
            "page.hta",
            "panel.cpl",
            "app.jar",
            "inst.msi",
            "tool.sh",
            "tool.command",
            "My.app",
            "pack.pkg",
            "pack.deb",
            "pack.rpm",
            "Tool.AppImage",
            "x.desktop",
            "m.docm",
            "macro.XLSM",
            "script.py",
            "Evil.EXE",
        ] {
            assert!(blocked(name).is_some(), "{name} must not open");
        }
    }

    #[test]
    fn the_message_names_the_kind_of_file() {
        assert_eq!(blocked("dir/Setup.EXE"), Some(".exe".to_owned()));
        assert_eq!(blocked("deploy.ps1"), Some(".ps1".to_owned()));
        assert_eq!(blocked("a.AppImage"), Some(".appimage".to_owned()));
    }

    #[test]
    fn a_program_cannot_hide_behind_trailing_dots_spaces_or_a_double_extension() {
        for name in [
            "evil.exe.",
            "evil.exe ",
            "evil.exe . .",
            "invoice.pdf.exe",
            "photo.png.scr",
        ] {
            assert!(blocked(name).is_some(), "{name} must not open");
        }
        // a harmless last extension is harmless, whatever comes before
        for name in ["setup.exe.txt", "run.sh.md", "a.bat.png"] {
            assert!(blocked(name).is_none(), "{name} may open");
        }
    }

    #[test]
    fn documents_pictures_sound_video_and_other_files_open() {
        for name in [
            "a.md",
            "a.pdf",
            "a.txt",
            "logo.png",
            "logo.svg",
            "clip.mp4",
            "song.mp3",
            "movie.mkv",
            "data.csv",
            "page.html",
            "photo.docx",
            "sheet.xlsx",
            "slides.pptx",
            "notes.odt",
            "archive.zip",
            "model.stl",
            "design.psd",
            "LICENSE",
            "Makefile",
            ".gitignore",
        ] {
            assert!(blocked(name).is_none(), "{name} may open");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_with_an_execute_permission_is_a_program_unless_it_is_a_document_or_media() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let make = |name: &str, mode: u32| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"x").expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
            path
        };
        for (name, program) in [
            ("gradlew", true),
            ("tool.bin", true),
            ("noext", true),
            ("clip.mp4", false),
            ("doc.pdf", false),
            ("readme.md", false),
        ] {
            let path = make(name, 0o755);
            assert_eq!(
                matches!(open_policy(&path), OpenPolicy::Blocked { .. }),
                program,
                "{name} with the execute permission"
            );
        }
        let plain = make("plain.bin", 0o644);
        assert_eq!(open_policy(&plain), OpenPolicy::Allowed);
    }

    #[test]
    fn media_extensions_are_openable_documents_are_too_programs_are_not() {
        for ext in ["png", "MP4", "flac", "md", "pdf", "svg"] {
            assert!(is_openable_extension(ext), "{ext}");
        }
        for ext in ["exe", "sh", "bin", "dll"] {
            assert!(!is_openable_extension(ext), "{ext}");
        }
    }
}
