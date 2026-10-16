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

/// The last component of a path written with `/` or `\`.
fn file_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The lower-case extension of a name exactly as it is written: the text after its last dot. `.gitignore` is a name, not
/// an extension, and `README.md.` has none (it ends in a dot).
fn exact_extension(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// The extension a path has on the system this program runs on. Only Windows drops the dots and spaces that end a name
/// (`evil.exe.` is `evil.exe` there); on macOS and Linux they are part of the name, and `README.md.` is a file without an
/// extension.
fn extension_of(path: &str) -> Option<String> {
    let name = file_name_of(path);
    if cfg!(windows) {
        exact_extension(name.trim_end_matches(['.', ' ']))
    } else {
        exact_extension(name)
    }
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

/// What a file is served as, when a request for `requested` was resolved (links followed) to the file `resolved`: its kind
/// and MIME type, from the name the file really has. A link named `a.png` that points at `.env` is not a picture, so nothing
/// is served through it; a link between two pictures is served as the picture it leads to.
pub fn served_as(requested: &str, resolved: &str) -> Option<(AssetKind, &'static str)> {
    lookup(requested)?;
    lookup(resolved)
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

/// The first `size` bytes of a file, `size` being what the file measured a moment ago. A file that grew meanwhile (a
/// recording still being written) does not make the answer longer than the limit the caller checked, and one that shrank
/// gives what is left.
pub fn read_whole(path: &Path, size: u64) -> io::Result<Vec<u8>> {
    match size.checked_sub(1) {
        Some(last) => read_part(path, 0, last),
        None => retry_read(|| std::fs::File::open(path).map(|_| Vec::new())),
    }
}

// ---- what may be opened by the system --------------------------------------------------------------------

/// Extensions of files that the system shows and never starts: documents, PDF files, office files, archives and the
/// like. Together with the pictures, sound and video, they are exempt from the execute-permission check of
/// [`open_policy`] (a Windows, exFAT or network drive often gives every file that permission).
const OPENABLE_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdown", "mkd", "mdx", "txt", "text", "log", "rst", "adoc", "org", "tex",
    "bib", "json", "jsonc", "yaml", "yml", "toml", "xml", "xsd", "csv", "tsv", "ini", "cfg",
    "conf", "html", "htm", "css", "pdf", "docx", "xlsx", "pptx", "odt", "ods", "odp", "rtf",
    "epub", "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz", "psd", "ai",
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
    "xbap",
    "msu",
    "vsix",
    "shs",
    "shb",
    "chm",
    "diagcab",
    "library-ms",
    "search-ms",
    "settingcontent-ms",
    "searchconnector-ms",
    "appcontent-ms",
    // Windows files that open a web page, a remote machine, a sandbox, a data link or an installer when opened
    "website",
    "theme",
    "themepack",
    "deskthemepack",
    "rdp",
    "appinstaller",
    "vsto",
    "wsb",
    "udl",
    // Access databases, which run macros when they open
    "mdb",
    "accdb",
    "mde",
    "accde",
    "ade",
    "adp",
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
    "msh",
    "msh1",
    "msh2",
    "mshxml",
    "msh1xml",
    "msh2xml",
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
    "pyzw",
    "pyo",
    "rbw",
    "ahk",
    "au3",
    // Office documents with macros, and the older Office formats that can hold them (their names say nothing about it)
    "doc",
    "dot",
    "xls",
    "xlt",
    "xla",
    "xlsb",
    "ppt",
    "pot",
    "pps",
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
    "tcsh",
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
    "inetloc",
    "fileloc",
    "afploc",
];

/// Whether `ext` (lower case, no dot) is one of the entries of a `PATHEXT` value (`.COM;.EXE;.BAT;...`).
fn in_path_ext(path_ext: &str, ext: &str) -> bool {
    path_ext
        .split(';')
        .map(|entry| entry.trim().trim_start_matches('.'))
        .any(|entry| !entry.is_empty() && entry.eq_ignore_ascii_case(ext))
}

/// Whether Windows itself runs files with this extension: the types in `%PATHEXT%`, read when asked, because the person can
/// add to it (`.PY`, `.PL`). Other systems have no such list.
fn is_path_ext(ext: &str) -> bool {
    cfg!(windows)
        && std::env::var_os("PATHEXT")
            .is_some_and(|value| in_path_ext(&value.to_string_lossy(), ext))
}

/// Whether the system starts a file with this extension (lower case, no dot) when it opens it, or may: the list above, the
/// types Windows runs, and the extensions that are a class ID in braces (`folder.{20D04FE0-...}`), which Windows reads as
/// a shell object whatever the file is.
fn is_program_extension(ext: &str) -> bool {
    PROGRAM_EXTENSIONS.contains(&ext) || ext.starts_with('{') || is_path_ext(ext)
}

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
    #[error("{label} files can run code (programs, scripts, installers, shortcuts, documents with macros) and are not opened from here")]
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
    // A program is known by either spelling of its name, whatever the system: as written (`evil.exe`) and without the
    // dots and spaces that end it (`evil.exe.`, which Windows runs as `evil.exe`). Refusing the second spelling on
    // a system that does not read it so costs nothing.
    let written = exact_extension(&name);
    let trimmed = exact_extension(name.trim_end_matches(['.', ' ']));
    for ext in [&written, &trimmed].into_iter().flatten() {
        if is_program_extension(ext) {
            return OpenPolicy::Blocked {
                label: format!(".{ext}"),
            };
        }
    }
    // Being a document earns the exemption from the execute check only by the name as written: `README.md.` is not a
    // `.md` file on macOS and Linux, and with an execute permission it is a script that `open` would run.
    let known_safe = written.as_deref().is_some_and(is_openable_extension);
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
            let name = format!("f.{ext}");
            assert_eq!(served_as(&name, &name), Some((*kind, *mime)));
        }
        assert_eq!(
            served_as("a.mp4", "a.mp4"),
            Some((AssetKind::Video, "video/mp4"))
        );
        assert_eq!(
            served_as("a.svg", "a.svg"),
            Some((AssetKind::Image, "image/svg+xml"))
        );
        assert_eq!(served_as("a.txt", "a.txt"), None);
    }

    /// Only Windows drops the dots and spaces that end a name: there `clip.mp4.` is `clip.mp4` and `evil.exe ` is `evil.exe`.
    /// On macOS and Linux they belong to the name, and `README.md.` is a file without an extension.
    #[cfg(windows)]
    #[test]
    fn on_windows_a_name_ended_by_dots_or_spaces_is_judged_without_them() {
        assert_eq!(asset_kind("clip.mp4."), Some(AssetKind::Video));
        assert_eq!(asset_kind("clip.mp4 . "), Some(AssetKind::Video));
        assert_eq!(extension_of("evil.exe. "), Some("exe".to_owned()));
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_a_name_ended_by_a_dot_or_a_space_has_no_extension() {
        assert_eq!(asset_kind("clip.mp4."), None);
        assert_eq!(asset_kind("clip.mp4 "), None);
        assert_eq!(extension_of("README.md."), None);
        assert_eq!(extension_of("evil.exe "), Some("exe ".to_owned()));
    }

    #[test]
    fn the_extension_as_written_never_drops_anything() {
        assert_eq!(exact_extension("README.md."), None);
        assert_eq!(exact_extension("a.tar.gz"), Some("gz".to_owned()));
        assert_eq!(exact_extension(".gitignore"), None);
        assert_eq!(exact_extension("noext"), None);
        assert_eq!(exact_extension("Photo.JPG"), Some("jpg".to_owned()));
    }

    #[test]
    fn a_file_is_served_as_the_file_a_link_leads_to() {
        // a link named like a picture that points at anything else serves nothing
        for resolved in [
            ".env",
            "secrets.txt",
            "id_rsa",
            "key.pem",
            "notes.md",
            "run.exe",
            "",
        ] {
            assert_eq!(served_as("a.png", resolved), None, "{resolved}");
            assert_eq!(served_as("clip.mp4", resolved), None, "{resolved}");
        }
        // the plain case, and a link between two pictures (served as the one it leads to)
        assert_eq!(
            served_as("logo.png", "logo.png"),
            Some((AssetKind::Image, "image/png"))
        );
        assert_eq!(
            served_as("cover.png", "covers/2026.JPG"),
            Some((AssetKind::Image, "image/jpeg"))
        );
        assert_eq!(
            served_as("latest.mp4", "runs/take-3.webm"),
            Some((AssetKind::Video, "video/webm"))
        );
        // and a request that does not name a picture is not served, whatever it leads to
        assert_eq!(served_as("notes.txt", "logo.png"), None);
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

    #[test]
    fn the_whole_file_is_the_size_it_measured_even_if_it_grew() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("growing.wav");
        std::fs::write(&file, b"0123456789").expect("write");
        // measured at 4 bytes, then written to: the answer stays within what was measured
        assert_eq!(read_whole(&file, 4).expect("grown"), b"0123");
        assert_eq!(read_whole(&file, 10).expect("same"), b"0123456789");
        // shrunk: what is left
        assert_eq!(read_whole(&file, 50).expect("shrunk"), b"0123456789");
        // an empty file, and a file that is gone
        let empty = dir.path().join("empty.wav");
        std::fs::write(&empty, b"").expect("write");
        assert_eq!(read_whole(&empty, 0).expect("empty"), Vec::<u8>::new());
        assert!(read_whole(&dir.path().join("gone.wav"), 0).is_err());
        assert!(read_whole(&dir.path().join("gone.wav"), 5).is_err());
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
            "app.xbap",
            "kb.msu",
            "ext.vsix",
            "x.shs",
            "x.msh1",
            "run.tcsh",
        ] {
            assert!(blocked(name).is_some(), "{name} must not open");
        }
    }

    #[test]
    fn the_types_that_open_a_page_a_machine_a_database_or_a_script_host_are_never_opened() {
        for ext in [
            // Windows files that open a page, a remote machine, a sandbox, a data link, an installer or a theme
            "website",
            "theme",
            "themepack",
            "deskthemepack",
            "rdp",
            "appinstaller",
            "vsto",
            "searchconnector-ms",
            "appcontent-ms",
            "wsb",
            "udl",
            "scf",
            "library-ms",
            "appref-ms",
            "settingcontent-ms",
            "chm",
            "msc",
            "cpl",
            "hta",
            // Access
            "mdb",
            "accdb",
            "mde",
            "accde",
            "ade",
            "adp",
            // scripts
            "pyzw",
            "pyo",
            "pyw",
            "py",
            "rbw",
            "rb",
            "pl",
            "ahk",
            "au3",
            // macOS
            "inetloc",
            "fileloc",
            "afploc",
            "terminal",
            "command",
            "tool",
            "workflow",
        ] {
            for name in [format!("x.{ext}"), format!("X.{}", ext.to_uppercase())] {
                assert_eq!(
                    blocked(&name),
                    Some(format!(".{ext}")),
                    "{name} must not open"
                );
            }
        }
    }

    #[test]
    fn the_older_office_formats_that_can_hold_macros_are_refused_with_the_macro_ones() {
        for ext in [
            "doc", "xls", "ppt", "pps", "xlsb", "xla", "dot", "xlt", "pot", "docm", "xlsm", "pptm",
        ] {
            assert_eq!(
                blocked(&format!("report.{ext}")),
                Some(format!(".{ext}")),
                "{ext}"
            );
        }
        // the formats that cannot hold macros open as ever
        for name in ["report.docx", "sheet.xlsx", "slides.pptx", "a.odt", "a.rtf"] {
            assert_eq!(blocked(name), None, "{name}");
        }
    }

    #[test]
    fn an_extension_that_is_a_class_id_is_refused_whatever_it_says() {
        for name in [
            "Documents.{FF393560-C2A7-11CF-BFF4-444553540000}",
            "folder.{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
            "notes.txt.{00000000-0000-0000-0000-000000000000}",
            "x.{not-even-an-id}",
        ] {
            assert!(blocked(name).is_some(), "{name} must not open");
        }
        // a brace elsewhere in the name is only a character
        assert_eq!(blocked("{draft} notes.md"), None);
    }

    #[test]
    fn the_types_windows_runs_come_from_pathext_as_it_is_set() {
        let pathext = ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.PY;.PYW;.PL";
        for ext in ["exe", "py", "pl", "msc", "COM", "Wsh"] {
            assert!(in_path_ext(pathext, ext), "{ext}");
        }
        for ext in ["md", "png", "pdf", "pyx", "e"] {
            assert!(!in_path_ext(pathext, ext), "{ext}");
        }
        // what a person adds, with or without the dot, with stray spaces, empty entries and a trailing separator
        assert!(in_path_ext(".EXE; .CPLX ;;ZZZ;", "cplx"));
        assert!(in_path_ext(".EXE; .CPLX ;;ZZZ;", "zzz"));
        assert!(!in_path_ext("", "exe"));
        assert!(!in_path_ext(";;", ""));
    }

    /// Windows reads `%PATHEXT%` when it is asked: every type in it is refused, whatever else is on the list.
    #[cfg(windows)]
    #[test]
    fn on_windows_every_type_of_the_pathext_of_this_system_is_refused() {
        let pathext = std::env::var("PATHEXT").expect("Windows sets PATHEXT");
        let entries: Vec<&str> = pathext
            .split(';')
            .map(|e| e.trim().trim_start_matches('.'))
            .filter(|e| !e.is_empty())
            .collect();
        assert!(entries.len() >= 5, "{pathext}");
        for ext in entries {
            assert!(
                blocked(&format!("tool.{ext}")).is_some(),
                ".{ext} is in PATHEXT"
            );
            assert!(blocked(&format!("tool.{}", ext.to_ascii_lowercase())).is_some());
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_pathext_means_nothing() {
        assert!(!is_path_ext("exe"));
        assert!(!is_path_ext("py"));
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

    /// Only Windows ignores the dots and spaces that end a name. On macOS and Linux `README.md.` is not a `.md` file: with an
    /// execute permission and a shebang it is a script that `open` or `xdg-open` would run, so it gets no exemption.
    #[cfg(unix)]
    #[test]
    fn a_name_that_ends_in_a_dot_or_a_space_does_not_earn_the_exemption_of_a_document() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let make = |name: &str, mode: u32| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"#!/bin/sh\necho no\n").expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
            path
        };
        for name in [
            "doc.pdf.",
            "README.md.",
            "notes.txt ",
            "clip.mp4. ",
            "photo.png..",
        ] {
            let path = make(name, 0o755);
            assert_eq!(
                open_policy(&path),
                OpenPolicy::Blocked {
                    label: name.to_owned()
                },
                "{name:?} with the execute permission"
            );
        }
        // the same names without the permission are plain files, and the names as written keep their exemption
        for (name, mode) in [
            ("doc.pdf.", 0o644),
            ("README.md.", 0o600),
            ("doc.pdf", 0o755),
        ] {
            let path = make(&format!("again-{name}"), mode);
            assert_eq!(
                open_policy(&path),
                OpenPolicy::Allowed,
                "{name} with mode {mode:o}"
            );
        }
        // a program is a program in either spelling
        let path = make("evil.exe.", 0o644);
        assert!(matches!(open_policy(&path), OpenPolicy::Blocked { .. }));
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
