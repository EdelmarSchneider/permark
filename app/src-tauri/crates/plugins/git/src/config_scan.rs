//! Whether the files git (the program, libgit2) reads from a repository's git folder keep it on this computer: the
//! config files (with everything they include) and the alternates. Read here as files, before either backend opens
//! the repository: both follow every path these files name, also one on a share (even a look connects to the server
//! and offers it the person's sign-in). No flag | option switches the repository's includes off in either backend.
//!
//! A config file without any of the watched names is fine as it is (no parsing, so nothing git could read
//! differently). Otherwise it is read with git's own grammar; anything this side cannot be sure both backends read
//! alike in a watched value (an escape, a continued line, a key without a value) refuses the repository, as does any
//! line git itself would refuse.

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
        }
    }

    /// The own metadata of `path` (a link is not followed), kept in [`Self::seen`].
    pub(crate) fn note(&mut self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        let meta = std::fs::symlink_metadata(path);
        // a file read right after its walk was noted is noted once
        if self.seen.0.last().is_none_or(|(last, _)| last != path) {
            let held = meta.as_ref().ok().and_then(stamp);
            self.seen.0.push((path.to_path_buf(), held));
        }
        meta
    }

    /// [`named_path`] of `named` against `base`, every name it walks below a local `base` kept in [`Self::seen`] (a
    /// link put on the way later is a change).
    pub(crate) fn resolve(&mut self, base: &Path, named: &str) -> Option<PathBuf> {
        let path = named_path(base, named)?;
        let relative = Path::new(named);
        if !relative.is_absolute() && is_local_path(base) {
            let mut at = base.to_path_buf();
            for part in relative.components() {
                at.push(part);
                let _ = self.note(&at);
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
        self.config(top, file, 0)
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
                let Some(value) = entry.plain_value() else {
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
        self.alternates(objects, 0, &mut listed)
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
    value: Option<Value>,
}

#[derive(Debug, PartialEq, Eq)]
struct Value {
    bytes: Vec<u8>,
    /// A backslash took part (an escape | a continued line).
    escaped: bool,
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

    /// The value as text, when both backends surely read it alike: present, UTF-8, no backslash.
    fn plain_value(&self) -> Option<&str> {
        let value = self.value.as_ref().filter(|value| !value.escaped)?;
        std::str::from_utf8(&value.bytes).ok()
    }
}

/// The characters of a config file as git reads them: `\r\n` is one line end, the end of the file is a last one.
struct Chars<'a> {
    bytes: &'a [u8],
    at: usize,
    eof: bool,
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

/// The entries of a config file, read with git's grammar (sections `[name]`, `[name "subsection"]`, `[name.sub]`, a
/// key on the header's own line, `;` | `#` comments, quotes, escapes, continued lines); `None`: a line git refuses.
fn entries(bytes: &[u8]) -> Option<Vec<Entry>> {
    let mut chars = Chars {
        bytes: bytes.strip_prefix(BOM).unwrap_or(bytes),
        at: 0,
        eof: false,
    };
    let mut section = Vec::new();
    let mut comment = false;
    let mut found = Vec::new();
    loop {
        let c = chars.next();
        if c == b'\n' {
            if chars.eof {
                return Some(found);
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
    loop {
        let mut c = chars.next();
        if c == b'\n' {
            return None;
        }
        if c == b'"' {
            break;
        }
        if c == b'\\' {
            c = chars.next();
            if c == b'\n' {
                return None;
            }
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
    let value = if c == b'\n' {
        None
    } else if c == b'=' {
        Some(value(chars)?)
    } else {
        return None;
    };
    Some(Entry {
        section: section.to_vec(),
        key,
        value,
    })
}

/// A value after its `=`, up to the end of its line: outer whitespace dropped, quotes taken out, a comment cut,
/// git's escapes read (`\\`, `\"`, `\n`, `\t`, `\b`, a line end: the value goes on); `None`: an escape git refuses | a
/// line end inside quotes.
fn value(chars: &mut Chars<'_>) -> Option<Value> {
    let (mut quote, mut comment, mut escaped) = (false, false, false);
    let mut space = 0;
    let mut bytes = Vec::new();
    loop {
        let mut c = chars.next();
        if c == b'\n' {
            return (!quote).then_some(Value { bytes, escaped });
        }
        if comment {
            continue;
        }
        if is_space(c) && !quote {
            if !bytes.is_empty() {
                space += 1;
            }
            continue;
        }
        if !quote && (c == b';' || c == b'#') {
            comment = true;
            continue;
        }
        bytes.extend(std::iter::repeat_n(b' ', space));
        space = 0;
        if c == b'\\' {
            escaped = true;
            c = match chars.next() {
                b'\n' => continue,
                b't' => b'\t',
                b'b' => 0x08,
                b'n' => b'\n',
                c @ (b'\\' | b'"') => c,
                _ => return None,
            };
            bytes.push(c);
            continue;
        }
        if c == b'"' {
            quote = !quote;
            continue;
        }
        bytes.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_of(section: &str, key: &str, value: Option<(&str, bool)>) -> Entry {
        Entry {
            section: section.as_bytes().to_vec(),
            key: key.as_bytes().to_vec(),
            value: value.map(|(text, escaped)| Value {
                bytes: text.as_bytes().to_vec(),
                escaped,
            }),
        }
    }

    /// The grammar of git's own config reader: what it takes, and what it refuses.
    #[test]
    fn config_files_are_read_as_git_reads_them() {
        let read = |text: &str| entries(text.as_bytes());
        assert_eq!(
            read("\u{feff}[Core]\n\tWorkTree = ..\r\n[core \"x\"] excludesFile=a b  ; c\n# [include]\n"),
            Some(vec![
                entry_of("core", "worktree", Some(("..", false))),
                entry_of("core", "excludesfile", Some(("a b", false))),
            ])
        );
        assert_eq!(
            read("[includeIf \"gitdir:~/x\\\"y\"]\npath = \" q ; # \"tail\n[include.sub]path\n"),
            Some(vec![
                entry_of("includeif", "path", Some((" q ; # tail", false))),
                entry_of("include", "path", None),
            ])
        );
        assert_eq!(
            read("[a]\nk = x\\\\y\\\"z\\n\\t\\b\nl = one\\\n two\n"),
            Some(vec![
                entry_of("a", "k", Some(("x\\y\"z\n\t\u{8}", true))),
                entry_of("a", "l", Some(("one two", true))),
            ])
        );
        assert_eq!(
            read("k = v\n"),
            Some(vec![entry_of("", "k", Some(("v", false)))])
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
            format!("[core]\n\texcludesFile = {local}\n\tattributesFile = \"{local}\"\n"),
            "[core]\n\tworktree = ../..\n[diff]\n\torderFile = order\n[mailmap]\n\tfile = ~/m\n".to_owned(),
            "[include]\n\tpath = missing.cfg\n".to_owned(),
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
