//! Whether the files git (the program, libgit2) reads from a repository's git folder keep it on this computer: the
//! config files (with everything they include) and the alternates. Read here as files, before either backend opens
//! the repository: both follow every path these files name, also one on a share (even a look connects to the server
//! and offers it the person's sign-in). No flag | option switches the repository's includes off in either backend.
//!
//! A config file without any of the watched names is fine as it is (no parsing, so nothing git could read
//! differently). Otherwise it is read with the part of git's grammar libgit2 reads alike: a backslash anywhere (an
//! escape, a continued line: libgit2 goes on past a comment line, takes `\\"#` as a comment and miscounts a quote right
//! after a backslash), a quote left open in any value, `\v` | `\f` (libgit2's whitespace, not git's) or a lone `\r`
//! refuses the repository, as do a watched key without a value, a watched value the two read differently and any line
//! git itself would refuse. Without a backslash both count quotes alike, so a comment starts at the same place for
//! both, also around a quoted value (git quotes a value holding `#` | `;`). A harmless value can hide a watched key
//! from one of them, so every value counts, not only the watched ones.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use permark_core::fsx::is_local_path;

use crate::worktrees::{named_path, read_git_file};

/// Most bytes of a config | alternates file read: the git folder came with the project.
const MAX_TEXT: u64 = 256 * 1024;
/// Deepest include git and libgit2 follow (both stop there).
const MAX_INCLUDE_DEPTH: usize = 10;
/// Most config files one look reads (includes may fan out).
const MAX_CONFIG_FILES: usize = 64;
/// Deepest alternates git and libgit2 follow (an alternate's own alternates, and so on).
const MAX_ALTERNATE_DEPTH: usize = 5;
/// Most alternates one look follows.
const MAX_ALTERNATES: usize = 32;
/// Longest watched value | alternates line taken: git writes one path there; a longer one is refused before any name
/// of it is looked up.
const MAX_VALUE: usize = 4 * 1024;
/// Most paths one look keeps for its answer ([`Seen`]); a look that would keep more refuses.
const MAX_SEEN: usize = 4096;
/// Names whose presence anywhere in a config file needs a closer look (ASCII, compared without case).
const WATCHED: [&[u8]; 6] = [
    b"include",
    b"worktree",
    b"excludesfile",
    b"attributesfile",
    b"orderfile",
    b"mailmap",
];
const BOM: &[u8] = b"\xef\xbb\xbf";

#[cfg(test)]
thread_local! {
    /// Git folders looked at on this thread (tests: an unchanged folder is looked at once).
    pub(crate) static SCANS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How a path looked: link, folder, size and modification time (ns); `None`: it could not be looked at.
type Stamp = Option<(bool, bool, u64, u128)>;

fn stamp(meta: &std::fs::Metadata) -> Stamp {
    let time = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((
        meta.file_type().is_symlink(),
        meta.is_dir(),
        meta.len(),
        time.as_nanos(),
    ))
}

/// Every path a look at a git folder went by, each as it looked right before: while they all look the same, the
/// answer stays the same.
#[derive(Debug, Default)]
pub(crate) struct Seen(Vec<(PathBuf, Stamp)>);

impl Seen {
    /// Whether every path still looks as it did (a link | folder never followed).
    pub(crate) fn unchanged(&self) -> bool {
        self.0.iter().all(|(path, held)| {
            std::fs::symlink_metadata(path)
                .ok()
                .and_then(|meta| stamp(&meta))
                == *held
        })
    }
}

/// What a file of the git folder holds.
enum Text {
    /// Nothing there: git reads nothing either.
    Missing,
    /// Something git reads whole but this side may not: a link, a FIFO, too large, a NUL byte.
    Unreadable,
    Bytes(Vec<u8>),
}

/// One look at the files of a git folder.
pub(crate) struct Scan {
    /// Where `~/` leads: every folder git | libgit2 may take as the home folder.
    homes: Vec<PathBuf>,
    /// Config files read so far.
    files: usize,
    pub(crate) seen: Seen,
    /// A path went unnoted ([`MAX_SEEN`]): the answer could not be kept, so it is a refusal.
    full: bool,
}

impl Scan {
    #[must_use]
    pub(crate) fn new() -> Self {
        #[cfg(test)]
        SCANS.with(|c| c.set(c.get() + 1));
        Self::with_homes(homes())
    }

    fn with_homes(homes: Vec<PathBuf>) -> Self {
        Self {
            homes,
            files: 0,
            seen: Seen::default(),
            full: false,
        }
    }

    /// The own metadata of `path` (a link is not followed), kept in [`Self::seen`]. `Err` without a look once
    /// [`MAX_SEEN`] paths are kept.
    pub(crate) fn note(&mut self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        // a file read right after its walk was noted is noted once
        let fresh = self.seen.0.last().is_none_or(|(last, _)| last != path);
        if fresh && self.seen.0.len() >= MAX_SEEN {
            self.full = true;
            return Err(std::io::Error::other("too many paths for one look"));
        }
        let meta = std::fs::symlink_metadata(path);
        if fresh {
            let held = meta.as_ref().ok().and_then(stamp);
            self.seen.0.push((path.to_path_buf(), held));
        }
        meta
    }

    /// [`named_path`] of `named` against `base`, the names it walks below a local `base` kept in [`Self::seen`] up to
    /// the first missing one (a link put on the way later is a change; so is the missing name appearing).
    pub(crate) fn resolve(&mut self, base: &Path, named: &str) -> Option<PathBuf> {
        let path = named_path(base, named)?;
        let relative = Path::new(named);
        if !relative.is_absolute() && is_local_path(base) {
            let mut at = base.to_path_buf();
            for part in relative.components() {
                at.push(part);
                if self.note(&at).is_err() {
                    break;
                }
            }
        }
        Some(path)
    }

    /// The bytes of the file `path` of the git folder ([`read_git_file`], at most [`MAX_TEXT`]).
    fn text(&mut self, path: &Path) -> Text {
        match self.note(path) {
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                return Text::Missing
            }
            Ok(meta) if meta.is_file() => {}
            _ => return Text::Unreadable,
        }
        match read_git_file(path, MAX_TEXT) {
            Some(bytes) if !bytes.contains(&0) => Text::Bytes(bytes),
            _ => Text::Unreadable,
        }
    }

    /// Whether the config file `file` of the git folder `top`, and every file it includes, names only places on this
    /// computer for the paths git | libgit2 read ([`Watched`]). A missing file is fine (git reads none).
    pub(crate) fn config_stays_local(&mut self, top: &Path, file: &Path) -> bool {
        self.config(top, file, 0) && !self.full
    }

    fn config(&mut self, top: &Path, file: &Path, depth: usize) -> bool {
        let bytes = match self.text(file) {
            Text::Missing => return true,
            Text::Unreadable => return false,
            Text::Bytes(bytes) => bytes,
        };
        self.files += 1;
        if depth > MAX_INCLUDE_DEPTH || self.files > MAX_CONFIG_FILES {
            return false;
        }
        if !WATCHED.iter().any(|word| holds(&bytes, word)) {
            return true;
        }
        let Some(entries) = entries(&bytes) else {
            return false;
        };
        let folder = file.parent().unwrap_or(top);
        entries.iter().all(|entry| match entry.watched() {
            None => true,
            Some(kind) => {
                let Some(value) = entry.plain_value().filter(|v| v.len() <= MAX_VALUE) else {
                    return false;
                };
                match kind {
                    // relative to the including file, as both backends read it
                    Watched::Include => self.places(folder, value).is_some_and(|places| {
                        places
                            .iter()
                            .all(|place| self.config(top, place, depth + 1))
                    }),
                    Watched::Place => self.places(top, value).is_some(),
                }
            }
        })
    }

    /// Where a watched value leads: `~/` against every home folder, anything else against `base` ([`named_path`]).
    /// `None`: a place that may not be looked up, `~user/` (another person's home) | `%(prefix)/` (git's own folder),
    /// which only git reads that way.
    fn places(&mut self, base: &Path, value: &str) -> Option<Vec<PathBuf>> {
        if value.starts_with("%(") {
            return None;
        }
        let Some(rest) = value.strip_prefix('~') else {
            return self.resolve(base, value).map(|place| vec![place]);
        };
        let rest = rest.strip_prefix('/')?;
        if self.homes.is_empty() {
            return None;
        }
        let homes = self.homes.clone();
        homes
            .iter()
            .map(|home| {
                is_local_path(home)
                    .then(|| self.resolve(home, rest))
                    .flatten()
            })
            .collect()
    }

    /// Whether the alternates of the objects folder `objects` (`info/alternates`, and theirs in turn) name only
    /// folders on this computer. A line neither relative to the objects folder (`.`) nor absolute is refused: libgit2
    /// reads it against its own current folder, git against the objects folder.
    pub(crate) fn alternates_stay_local(&mut self, objects: &Path) -> bool {
        let mut listed = 0;
        self.alternates(objects, 0, &mut listed) && !self.full
    }

    fn alternates(&mut self, objects: &Path, depth: usize, listed: &mut usize) -> bool {
        // name by name: a link at `info` would lead the read anywhere
        let Some(file) = self.resolve(objects, "info/alternates") else {
            return false;
        };
        let bytes = match self.text(&file) {
            Text::Missing => return true,
            Text::Unreadable => return false,
            Text::Bytes(bytes) => bytes,
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return false;
        };
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            *listed += 1;
            // a quoted line is unquoted by git, read as it is by libgit2
            if *listed > MAX_ALTERNATES
                || depth > MAX_ALTERNATE_DEPTH
                || line.len() > MAX_VALUE
                || line.starts_with('"')
                || !(line.starts_with('.') || Path::new(line).is_absolute())
            {
                return false;
            }
            let Some(folder) = self.resolve(objects, line) else {
                return false;
            };
            if !self.alternates(&folder, depth + 1, listed) {
                return false;
            }
        }
        true
    }
}

/// Every folder git | libgit2 may take as the home folder for `~/`: `HOME` first for both, then the system's own
/// answer (and on Windows the profile variables libgit2 falls back to).
fn homes() -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = std::env::var_os("HOME")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    #[cfg(windows)]
    {
        if let (Some(drive), Some(path)) =
            (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
        {
            let mut joined = drive;
            joined.push(path);
            homes.push(PathBuf::from(joined));
        }
        homes.extend(std::env::var_os("USERPROFILE").map(PathBuf::from));
    }
    homes.extend(dirs::home_dir());
    homes.sort();
    homes.dedup();
    homes
}

/// Whether `bytes` holds `word` (ASCII letters compared without case).
fn holds(bytes: &[u8], word: &[u8]) -> bool {
    bytes
        .windows(word.len())
        .any(|window| window.eq_ignore_ascii_case(word))
}

/// What a watched key leads git to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watched {
    /// Another config file, read at once (`include.path`, `includeIf.<condition>.path`).
    Include,
    /// A file | folder git reads | changes into (`core.worktree`, `core.excludesFile`, `core.attributesFile`,
    /// `diff.orderFile`, `mailmap.file`).
    Place,
}

/// One `key = value` of a config file as git reads it.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    /// The section name without its subsection, lowercase.
    section: Vec<u8>,
    /// Lowercase.
    key: Vec<u8>,
    /// `None`: the key alone (git takes it as true).
    value: Option<Vec<u8>>,
    /// libgit2 reads the same value (it keeps the spaces after an empty pair of quotes and a tab as it is, where git
    /// drops them | writes a space).
    alike: bool,
}

impl Entry {
    /// What the key leads git to, whatever the subsection (a stricter reading than git's, never a looser one).
    fn watched(&self) -> Option<Watched> {
        match (self.section.as_slice(), self.key.as_slice()) {
            (b"include" | b"includeif", b"path") => Some(Watched::Include),
            (b"core", b"worktree" | b"excludesfile" | b"attributesfile")
            | (b"diff", b"orderfile")
            | (b"mailmap", b"file") => Some(Watched::Place),
            _ => None,
        }
    }

    /// The value as text: present, read alike by both, UTF-8.
    fn plain_value(&self) -> Option<&str> {
        if !self.alike {
            return None;
        }
        std::str::from_utf8(self.value.as_deref()?).ok()
    }
}

/// The characters of a config file as git reads them: `\r\n` is one line end, the end of the file is a last one.
struct Chars<'a> {
    bytes: &'a [u8],
    at: usize,
    eof: bool,
    /// A byte went by that libgit2 reads unlike git somewhere: a backslash (a continued line goes on past a comment
    /// line there, `\\"#` starts a comment), `\v` | `\f` (whitespace there), a lone `\r` (inside its line there), NUL.
    odd: bool,
}

impl Chars<'_> {
    fn next(&mut self) -> u8 {
        let Some(&c) = self.bytes.get(self.at) else {
            self.eof = true;
            return b'\n';
        };
        self.at += 1;
        if c == b'\r' && self.bytes.get(self.at) == Some(&b'\n') {
            self.at += 1;
            return b'\n';
        }
        if matches!(c, b'\\' | b'\r' | 0x0b | 0x0c | 0) {
            self.odd = true;
        }
        c
    }
}

/// git's whitespace (no vertical tab, no form feed).
const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

const fn is_key_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
}

/// The entries of a config file, read with the part of git's grammar libgit2 reads alike (sections `[name]`,
/// `[name "subsection"]`, `[name.sub]`, a key on the header's own line, `;` | `#` comments, quoted values); `None`: a
/// line git refuses | a byte libgit2 reads otherwise ([`Chars::odd`]) | a quote left open in a value.
fn entries(bytes: &[u8]) -> Option<Vec<Entry>> {
    let mut chars = Chars {
        bytes: bytes.strip_prefix(BOM).unwrap_or(bytes),
        at: 0,
        eof: false,
        odd: false,
    };
    let mut section = Vec::new();
    let mut comment = false;
    let mut found = Vec::new();
    loop {
        let c = chars.next();
        if c == b'\n' {
            if chars.eof {
                return (!chars.odd).then_some(found);
            }
            comment = false;
            continue;
        }
        if comment || is_space(c) {
            continue;
        }
        if c == b'#' || c == b';' {
            comment = true;
            continue;
        }
        if c == b'[' {
            section = header(&mut chars)?;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            return None;
        }
        found.push(entry(&mut chars, &section, c)?);
    }
}

/// A section header after its `[`: the section name before its first dot (a subsection is not kept).
fn header(chars: &mut Chars<'_>) -> Option<Vec<u8>> {
    let mut name = Vec::new();
    loop {
        let c = chars.next();
        if chars.eof {
            return None;
        }
        if c == b']' {
            break;
        }
        if is_space(c) {
            subsection(chars, c)?;
            break;
        }
        if !is_key_char(c) && c != b'.' {
            return None;
        }
        name.push(c.to_ascii_lowercase());
    }
    if name.is_empty() {
        return None;
    }
    name.split(|&c| c == b'.').next().map(<[u8]>::to_vec)
}

/// The rest of `[name "subsection"]` after the space `c`, up to its `]`.
fn subsection(chars: &mut Chars<'_>, mut c: u8) -> Option<()> {
    loop {
        if c == b'\n' {
            return None;
        }
        c = chars.next();
        if !is_space(c) {
            break;
        }
    }
    if c != b'"' {
        return None;
    }
    // a backslash (an escape here) refuses the whole file: [`Chars::odd`]
    loop {
        match chars.next() {
            b'\n' => return None,
            b'"' => break,
            _ => {}
        }
    }
    (chars.next() == b']').then_some(())
}

/// A `key [= value]` line starting with the letter `first`.
fn entry(chars: &mut Chars<'_>, section: &[u8], first: u8) -> Option<Entry> {
    let mut key = vec![first.to_ascii_lowercase()];
    let mut c;
    loop {
        c = chars.next();
        if chars.eof || !is_key_char(c) {
            break;
        }
        key.push(c.to_ascii_lowercase());
    }
    while c == b' ' || c == b'\t' {
        c = chars.next();
    }
    let (value, alike) = if c == b'\n' {
        (None, true)
    } else if c == b'=' {
        let (value, alike) = value(chars)?;
        (Some(value), alike)
    } else {
        return None;
    };
    Some(Entry {
        section: section.to_vec(),
        key,
        value,
        alike,
    })
}

/// A value after its `=`, up to the end of its line, as git reads it: whitespace outside quotes dropped at the ends
/// (inside the value: a space each), a comment cut at a `;` | `#` outside quotes, the quotes removed. With it whether
/// libgit2 reads the same bytes: it takes the text from the first to the last byte that is no space before that
/// comment, quotes removed. `None`: a quote left open (git refuses the file, libgit2 reads on). Both count quotes alike
/// only where no backslash is, and a backslash (an escape, a continued line) refuses the whole file: [`Chars::odd`].
fn value(chars: &mut Chars<'_>) -> Option<(Vec<u8>, bool)> {
    let (mut quote, mut comment) = (false, false);
    let mut space = 0;
    let mut bytes = Vec::new();
    // libgit2's reading: every byte from the first that is no space; `kept` ends at the last that is no space
    let mut raw = Vec::new();
    let mut kept = 0;
    loop {
        let c = chars.next();
        if c == b'\n' {
            if quote {
                return None;
            }
            raw.truncate(kept);
            raw.retain(|&b| b != b'"');
            let alike = raw == bytes;
            return Some((bytes, alike));
        }
        if comment {
            continue;
        }
        if !quote && (c == b';' || c == b'#') {
            comment = true;
            continue;
        }
        if !is_space(c) || !raw.is_empty() {
            raw.push(c);
            if !is_space(c) {
                kept = raw.len();
            }
        }
        if is_space(c) && !quote {
            if !bytes.is_empty() {
                space += 1;
            }
            continue;
        }
        bytes.extend(std::iter::repeat_n(b' ', space));
        space = 0;
        if c == b'"' {
            quote = !quote;
        } else {
            bytes.push(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_of(section: &str, key: &str, value: Option<&str>) -> Entry {
        Entry {
            section: section.as_bytes().to_vec(),
            key: key.as_bytes().to_vec(),
            value: value.map(|text| text.as_bytes().to_vec()),
            alike: true,
        }
    }

    /// The part of git's own config grammar libgit2 reads alike: what it takes, and what it refuses (all git refuses,
    /// and escapes, continued lines, an open quote, bytes libgit2 reads otherwise).
    #[test]
    fn config_files_are_read_as_git_reads_them() {
        let read = |text: &str| entries(text.as_bytes());
        assert_eq!(
            read("\u{feff}[Core]\n\tWorkTree = ..\r\n[core \"x\"] excludesFile=a b  ; c\n# [include]\n"),
            Some(vec![
                entry_of("core", "worktree", Some("..")),
                entry_of("core", "excludesfile", Some("a b")),
            ])
        );
        assert_eq!(
            read("[includeIf \"gitdir:~/x y\"]\npath = q ; # tail\n[include.sub]path\n"),
            Some(vec![
                entry_of("includeif", "path", Some("q")),
                entry_of("include", "path", None),
            ])
        );
        assert_eq!(read("k = v\n"), Some(vec![entry_of("", "k", Some("v"))]));
        // quotes without a backslash in the file: kept spaces, no comment inside, a comment after
        assert_eq!(
            read("[a]\nk = \" q \"\n[b \"include#1\"]\nmerge = \"refs/heads/include#1\" ; \"c\nv = x ; \"\n"),
            Some(vec![
                entry_of("a", "k", Some(" q ")),
                entry_of("b", "merge", Some("refs/heads/include#1")),
                entry_of("b", "v", Some("x")),
            ])
        );
        // git drops the spaces after an empty pair of quotes, libgit2 keeps them
        assert_eq!(
            read("[a]\nk = \"\"  x\n"),
            Some(vec![Entry {
                alike: false,
                ..entry_of("a", "k", Some("x"))
            }])
        );
        assert_eq!(read(""), Some(Vec::new()));
        assert_eq!(read("[a]"), Some(Vec::new()));
        for refused in [
            "[core",
            "[core\n]",
            "[]",
            "[core \"x]\n",
            "[core \"x\" ]\n",
            "[core x]\n",
            "[co:re]\n",
            "[a]\n1k = v\n",
            "[a]\nk # v\n",
            "[a]\nk = \"open\n",
            "[a]\nk = \\x\n",
            "[a]\nk_y = v\n",
            "\u{fe}[a]\n",
            "[a \"x\\\"y\"]\n",
            "[a]\nk = x\\\\y\n",
            "[a]\nk = one\\\n two\n",
            "[a]\nk = a \"b ; c\n",
            "[a]\nk = \"x\\\"\"\n",
            "[a]\n# c\\\n",
            "[a]\nk = \u{b}v\n",
            "[a]\nk = v\u{c}\n",
            "[a]\nk = v\rw\n",
        ] {
            assert_eq!(read(refused), None, "{refused:?}");
        }
    }

    fn scan(homes: &[&Path]) -> Scan {
        Scan::with_homes(homes.iter().map(|home| home.to_path_buf()).collect())
    }

    fn slashed(path: &Path) -> String {
        path.display().to_string().replace('\\', "/")
    }

    /// A share no host answers (TEST-NET-1), as a config value.
    const SHARE: &str = "//192.0.2.1/s";

    #[test]
    fn watched_values_must_lead_to_places_here() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join(".git");
        std::fs::create_dir_all(&top).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let config = top.join("config");
        let local = slashed(&dir.path().join("x"));
        let check = |text: &str, homes: &[&Path]| {
            std::fs::write(&config, text).unwrap();
            scan(homes).config_stays_local(&top, &config)
        };
        for fine in [
            "[core]\n\tbare = false\n".to_owned(),
            // the watched words elsewhere: in a comment, another key, another value
            "# include\n[extensions]\n\tworktreeConfig = true\n[remote \"o\"]\n\turl = //h/mailmap\n".to_owned(),
            format!("[core]\n\texcludesFile = {local}\n\tattributesFile = {local}\n"),
            "[core]\n\tworktree = ../..\n[diff]\n\torderFile = order\n[mailmap]\n\tfile = ~/m\n".to_owned(),
            "[include]\n\tpath = missing.cfg\n".to_owned(),
            format!("[core]\n\tattributesFile = \"{local}\"\n"),
        ] {
            assert!(check(&fine, &[&home]), "{fine}");
        }
        for refused in [
            format!("[core]\n\texcludesFile = {SHARE}/x\n"),
            format!("[core]\n\tworktree = {SHARE}/w\n"),
            format!("[diff]\n\torderFile = {SHARE}/o\n"),
            format!("[mailmap]\n\tfile = {SHARE}/m\n"),
            format!("[core]\n\tattributesFile = {SHARE}/a\n"),
            format!("[core \"any\"]\n\texcludesFile = {SHARE}/x\n"),
            "[core]\n\texcludesFile\n".to_owned(),
            "[core]\n\texcludesFile = a\\\\b\n".to_owned(),
            "[core]\n\texcludesFile = ~other/x\n".to_owned(),
            "[core]\n\texcludesFile = ~\n".to_owned(),
            "[core]\n\texcludesFile = %(prefix)/x\n".to_owned(),
            "[core]\n\texcludesFile = x\0\n".to_owned(),
            "[core\n\texcludesFile = x\n".to_owned(),
            format!("[core]\n\texcludesFile = {}\n", "x".repeat(300 * 1024)),
        ] {
            assert!(!check(&refused, &[&home]), "{refused:.80}");
        }
        // `~/` needs every home folder to be here
        let far_home = Path::new(SHARE).join("home");
        assert!(!check(
            "[core]\n\texcludesFile = ~/x\n",
            &[&home, &far_home]
        ));
        assert!(!check("[core]\n\texcludesFile = ~/x\n", &[]));
        // not UTF-8
        std::fs::write(&config, b"[core]\n\texcludesFile = \xff\n").unwrap();
        assert!(!scan(&[&home]).config_stays_local(&top, &config));
        if cfg!(windows) {
            for refused in ["/x", "Z:x", "a?b"] {
                assert!(
                    !check(&format!("[core]\n\texcludesFile = {refused}\n"), &[]),
                    "{refused}"
                );
            }
        }
    }

    #[test]
    fn includes_are_followed_relative_to_the_including_file() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join("repo").join(".git");
        std::fs::create_dir_all(&top).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("cfg")).unwrap();
        let config = top.join("config");
        std::fs::write(&config, "[include]\n\tpath = ../../one.cfg\n").unwrap();
        std::fs::write(
            dir.path().join("one.cfg"),
            "[includeIf \"onbranch:x\"]\n\tpath = sub/two.cfg\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        let two = dir.path().join("sub").join("two.cfg");
        std::fs::write(&two, "[include]\n\tpath = ~/cfg/three.cfg\n").unwrap();
        let three = home.join("cfg").join("three.cfg");
        std::fs::write(&three, "[user]\n\tname = x\n").unwrap();
        assert!(scan(&[&home]).config_stays_local(&top, &config));
        // the last file of the chain names a share
        std::fs::write(&three, format!("[mailmap]\n\tfile = {SHARE}/m\n")).unwrap();
        assert!(!scan(&[&home]).config_stays_local(&top, &config));
        // an include naming a share | a cycle (deeper than git goes) | a folder in place of a file
        for text in [
            format!("[include]\n\tpath = {SHARE}/c\n"),
            "[include]\n\tpath = two.cfg\n".to_owned(),
            "[include]\n\tpath = .\n".to_owned(),
        ] {
            std::fs::write(&two, &text).unwrap();
            assert!(!scan(&[&home]).config_stays_local(&top, &config), "{text}");
        }
        // many includes of a fine file: more files than one look reads
        let many = "[include]\n\tpath = leaf.cfg\n".repeat(MAX_CONFIG_FILES + 1);
        std::fs::write(dir.path().join("sub").join("leaf.cfg"), "[a]\n\tb = c\n").unwrap();
        std::fs::write(&two, many).unwrap();
        assert!(!scan(&[&home]).config_stays_local(&top, &config));
    }

    #[test]
    fn alternates_must_be_relative_to_the_objects_folder_or_here() {
        let dir = tempfile::tempdir().unwrap();
        let objects = dir.path().join("repo").join(".git").join("objects");
        let info = objects.join("info");
        std::fs::create_dir_all(&info).unwrap();
        let other = dir.path().join("other").join("objects");
        std::fs::create_dir_all(other.join("info")).unwrap();
        let check = |text: &str| {
            std::fs::write(info.join("alternates"), text).unwrap();
            scan(&[]).alternates_stay_local(&objects)
        };
        assert!(scan(&[]).alternates_stay_local(&objects), "none");
        let absolute = slashed(&other);
        for fine in [
            "../../../other/objects\n".to_owned(),
            format!("# a comment\n\n{absolute}\r\n"),
        ] {
            assert!(check(&fine), "{fine}");
        }
        for refused in [
            format!("{SHARE}/objects\n"),
            "other/objects\n".to_owned(),
            format!("\"{absolute}\"\n"),
            "../x\n".repeat(MAX_ALTERNATES + 1),
            "x\0\n".to_owned(),
            // longer than any path git writes there
            format!("{}x\n", "../".repeat(2000)),
        ] {
            assert!(!check(&refused), "{refused:.80}");
        }
        // an alternate's own alternates are followed
        std::fs::write(info.join("alternates"), "../../../other/objects\n").unwrap();
        std::fs::write(
            other.join("info").join("alternates"),
            format!("{SHARE}/o\n"),
        )
        .unwrap();
        assert!(!scan(&[]).alternates_stay_local(&objects));
        // a chain deeper than git goes
        std::fs::write(other.join("info").join("alternates"), ".\n").unwrap();
        assert!(!scan(&[]).alternates_stay_local(&objects));
    }

    /// Inputs git and libgit2 read differently: libgit2 skips a comment | blank line after a continued value and goes
    /// on with the line after it, takes `\\"#` as the start of a comment, and drops `\v` | `\f` before a value. Each
    /// hides a watched key from git (and from a reading with git's grammar) that libgit2 reads, so a file holding a
    /// watched word and any of them is refused, whatever its values name.
    #[test]
    fn what_git_and_libgit2_may_read_differently_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join(".git");
        std::fs::create_dir_all(&top).unwrap();
        let config = top.join("config");
        let hidden: [(&str, &str); 6] = [
            (
                "[include]\n a = x\\\n#\n[core]\n path = {P}\n",
                "include.path",
            ),
            (
                "[include]\n a = x\\\n\n[core]\n path = {P}\n",
                "include.path",
            ),
            ("[include]\n a = \\\\\"#\\\n path = {P}\"\n", "include.path"),
            (
                "[core]\n a = x\\\n;\n[user]\n excludesFile = {P}\n",
                "core.excludesfile",
            ),
            ("[core]\n\texcludesFile = \u{b}{P}\n", "core.excludesfile"),
            ("[core]\n\texcludesFile = \u{c}{P}\n", "core.excludesfile"),
        ];
        for (text, key) in hidden {
            // libgit2 reads the watched key (a missing file of the folder here: nothing it could reach)
            std::fs::write(&config, text.replace("{P}", "missing.cfg")).unwrap();
            let read = git2::Config::open(&config).unwrap();
            assert_eq!(
                read.get_string(key).ok().as_deref(),
                Some("missing.cfg"),
                "{text:?}"
            );
            for named in ["missing.cfg", SHARE] {
                std::fs::write(&config, text.replace("{P}", named)).unwrap();
                assert!(
                    !scan(&[]).config_stays_local(&top, &config),
                    "{text:?} {named}"
                );
            }
        }
        // the same bytes anywhere in a file holding a watched word: a lone `\r`, a backslash in a header | a comment |
        // a quoted value (also one that is not watched)
        for refused in [
            "[core]\n\texcludesFile = x\ry\n",
            "[includeIf \"gitdir:a\\\\b\"]\n\tpath = x\n",
            "[core]\n\texcludesFile = x\n# a\\\n",
            "[core]\n\tworktree = ..\n[user]\n\tname = \"A\\\\B\"\n",
        ] {
            std::fs::write(&config, refused).unwrap();
            assert!(!scan(&[]).config_stays_local(&top, &config), "{refused:?}");
        }
        // what nearly every repository's config holds: a quoted subsection in a header is read alike by both
        std::fs::write(
            &config,
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = https://example.com/r.git\n\tfetch = \
             +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n\tmerge = \
             refs/heads/main\n[extensions]\n\tworktreeConfig = true\n",
        )
        .unwrap();
        assert!(scan(&[]).config_stays_local(&top, &config));
    }

    /// git writes a value holding `#` | `;` in quotes (a branch `include#1`: `merge = "refs/heads/include#1"`). Without a
    /// backslash in the file both readers count quotes alike (libgit2 miscounts only right after a backslash), so a
    /// comment starts at the same place for both: such a file is fine. A quoted watched value is checked as both read
    /// it; an open quote refuses (git refuses the file, libgit2 reads on), and so does a watched value the two would
    /// read differently.
    #[test]
    fn quoted_values_are_read_alike_without_a_backslash() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join(".git");
        std::fs::create_dir_all(&top).unwrap();
        let config = top.join("config");
        let check = |text: &str| {
            std::fs::write(&config, text).unwrap();
            scan(&[]).config_stays_local(&top, &config)
        };
        // as `git clone` writes it for a repository whose default branch is `include#1`
        let clone = "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[remote \"origin\"]\n\turl = \
                     https://example.com/r.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"include#1\"]\n\t\
                     remote = origin\n\tmerge = \"refs/heads/include#1\"\n[extensions]\n\tworktreeConfig = true\n";
        assert!(check(clone));
        let read = git2::Config::open(&config).unwrap();
        assert_eq!(
            read.get_string("branch.include#1.merge").unwrap(),
            "refs/heads/include#1"
        );
        let local = slashed(&dir.path().join("x"));
        for fine in [
            "[core]\n\tworktree = ..\n[user]\n\tname = \"A\"\n".to_owned(),
            "[core]\n\tworktree = .. ; \"x\n".to_owned(),
            format!("[core]\n\texcludesFile = \"{local}\" # a \"comment\n"),
            format!("[core]\n\texcludesFile = \"{}\"/x\n", slashed(dir.path())),
        ] {
            assert!(check(&fine), "{fine}");
        }
        // libgit2 reads the spaces after an empty pair of quotes, git drops them: two different files
        std::fs::write(&config, "[include]\n\tpath = \"\"  x.cfg\n").unwrap();
        let read = git2::Config::open(&config).unwrap();
        assert_eq!(read.get_string("include.path").unwrap(), "  x.cfg");
        for refused in [
            format!("[core]\n\texcludesFile = \"{SHARE}/x\"\n"),
            "[core]\n\texcludesFile = \"//192.0.2.1\"/s/x\n".to_owned(),
            format!("[include]\n\tpath = \"{SHARE}/x\" ; local.cfg\n"),
            "[core]\n\tworktree = ..\n[user]\n\tname = \"A\n".to_owned(),
            "[core]\n\tworktree = ..\n[user]\n\tname = a \"b ; c\n".to_owned(),
            "[include]\n\tpath = \"\"  x.cfg\n".to_owned(),
            "[core]\n\tworktree = ..\n[user]\n\tname = \"A\\\"B\"\n".to_owned(),
        ] {
            assert!(!check(&refused), "{refused}");
        }
    }

    /// A watched value is short in any config git writes: a long one (256 KiB of names) is refused before any of its
    /// names is looked up, and the paths a look keeps for its answer stay few (values under the cap whose names all
    /// exist: `..` climbs to the root and stays there).
    #[test]
    fn a_long_watched_value_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join(".git");
        std::fs::create_dir_all(&top).unwrap();
        let config = top.join("config");
        std::fs::write(
            &config,
            format!(
                "[core]\n\texcludesFile = {}\n",
                "a/".repeat(128 * 1024 - 32)
            ),
        )
        .unwrap();
        let started = std::time::Instant::now();
        let mut look = scan(&[]);
        assert!(!look.config_stays_local(&top, &config));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(look.seen.0.len() < 8, "{}", look.seen.0.len());
        // a whole path under the shortest path limit of the systems the app runs on (macOS: 1024 bytes) too: a
        // longer one is refused at its first look, before the cap is reached
        let names = 250;
        let climb = format!("[core]\n\texcludesFile = {}x\n", "../".repeat(names));
        std::fs::write(&config, climb.repeat(MAX_SEEN / names + 2)).unwrap();
        let mut look = scan(&[]);
        assert!(!look.config_stays_local(&top, &config));
        assert!(look.full);
        assert!(look.seen.0.len() <= MAX_SEEN, "{}", look.seen.0.len());
    }

    /// The paths a look went by keep its answer: the same stamps, the same answer; a change of any of them (also a
    /// file that appears) asks again.
    #[test]
    fn a_look_remembers_every_path_it_went_by() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join(".git");
        std::fs::create_dir_all(&top).unwrap();
        let config = top.join("config");
        std::fs::write(&config, "[include]\n\tpath = sub/inc.cfg\n").unwrap();
        let mut look = scan(&[]);
        assert!(look.config_stays_local(&top, &config));
        let seen: Vec<&Path> = look.seen.0.iter().map(|(path, _)| path.as_path()).collect();
        assert_eq!(
            seen,
            [
                config.as_path(),
                &top.join("sub"),
                &top.join("sub").join("inc.cfg")
            ]
        );
        assert!(look.seen.unchanged());
        std::fs::create_dir_all(top.join("sub")).unwrap();
        assert!(!look.seen.unchanged());
    }
}
