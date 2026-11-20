//! Reads of files whose bytes someone else may have placed: a project's `.permark/` arrives with the folder (clone,
//! zip, sync partner), so anything may stand where a file is expected. Only a plain file is read: never a FIFO or a
//! device (a read of one can wait forever), never more than a cap (checked before the bytes are taken in), and in a
//! zone never through a link.

use std::fs::Metadata;
use std::io::{self, Read};
use std::path::Path;

use super::{opened_path, retry, FsxError, RETRY_DELAYS};

/// Where the file a read opens may really be.
#[derive(Debug, Clone, Copy)]
pub enum Zone<'a> {
    /// Below this folder (a project's `.permark/`): neither the folder, nor a folder on the way, nor the name read is a
    /// link, each looked at top-down before anything below it is looked up (a link followed during the lookup could
    /// reach a network share: an outbound connection, a long wait); and the file the handle opened sits below the
    /// folder (a link swapped in right before the open never leads out of it).
    Inside(&'a Path),
    /// Wherever links lead: the person's own profile, which dotfile managers and folder redirection link elsewhere.
    Anywhere,
}

/// Reads the plain file at `path`, at most `limit` bytes, where `zone` allows. Windows opens it as any reader would
/// (a cloud placeholder is fetched, not read as its stub); unix opens it without waiting for a writer, so a FIFO is
/// refused at once.
///
/// # Errors
/// `TooLarge` (checked before reading); `Io` `InvalidData` for a link (in a zone), a folder, FIFO or device, or a file
/// opened outside the zone (not transient: never retried); `Io` `NotFound` when nothing is there; any other IO failure.
pub fn read_regular(path: &Path, limit: u64, zone: Zone<'_>) -> Result<Vec<u8>, FsxError> {
    let read = retry(&RETRY_DELAYS, || {
        let named = match zone {
            Zone::Inside(folder) => symlink_metadata_below(folder, path)?,
            Zone::Anywhere => std::fs::metadata(path)?,
        };
        // before the open: Windows answers a folder's open with access denied, which would be retried
        if !named.file_type().is_file() {
            return Err(refused("a link or not a plain file"));
        }
        let file = open(path, zone)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(refused("not a plain file"));
        }
        if let Zone::Inside(folder) = zone {
            // `None`: the system cannot tell; the name itself was checked above
            if opened_path(&file).is_some_and(|opened| !below(&opened, folder)) {
                return Err(refused("outside its folder"));
            }
        }
        if meta.len() > limit {
            return Ok(None);
        }
        let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
        // `take`: a file growing while read never exceeds the limit
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    })
    .map_err(|e| FsxError::io(path, e))?;
    match read {
        Some(bytes) if bytes.len() as u64 <= limit => Ok(bytes),
        _ => Err(FsxError::TooLarge {
            path: path.display().to_string(),
            limit,
        }),
    }
}

/// Whether an entry (its own metadata, not followed) is a link a zone walk refuses: only what std reports as one, a
/// symlink, or on Windows a name-surrogate reparse point (symlink, junction). Not any reparse point: a cloud
/// placeholder (OneDrive Files On-Demand) carries one too and is a plain folder | file, so the state of a project in a
/// synced folder stays readable and writable.
pub(crate) fn is_link(meta: &Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// `symlink_metadata` of `path` below `folder`, never through a link ([`is_link`]): `folder` and each folder on the way
/// are looked at first, top-down, and a link is refused before anything below it is looked up. Under a plain file
/// nothing can be: the system answers for the rest, following nothing.
///
/// # Errors
/// `InvalidData` for a link on the way or a path that is not below `folder`; `NotFound` when nothing is there; any
/// other IO failure.
pub(crate) fn symlink_metadata_below(folder: &Path, path: &Path) -> io::Result<Metadata> {
    // the folder itself first, as named, before its spelling is resolved: resolving follows a link there
    let mut meta = std::fs::symlink_metadata(folder)?;
    if is_link(&meta) {
        return Err(refused("a link on the way"));
    }
    let (mut at, names) = if let Some(names) = crate::paths::strip_root(path, folder) {
        (folder.to_path_buf(), names)
    } else {
        // a folder named in another spelling than the system's own (an 8.3 name)
        let canon = crate::paths::canonicalize(folder)?;
        let names =
            crate::paths::strip_root(path, &canon).ok_or_else(|| refused("outside its folder"))?;
        (canon, names)
    };
    let Some((_, folders)) = names.split_last() else {
        return Err(refused("outside its folder"));
    };
    if names.iter().any(|name| *name == "..") {
        return Err(refused("outside its folder"));
    }
    for step in folders {
        if !meta.is_dir() {
            break;
        }
        at.push(step);
        meta = std::fs::symlink_metadata(&at)?;
        if is_link(&meta) {
            return Err(refused("a link on the way"));
        }
    }
    std::fs::symlink_metadata(path)
}

/// A file left unread on purpose: `InvalidData` (the caller names the file).
fn refused(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

#[cfg(unix)]
fn open(path: &Path, zone: Zone<'_>) -> io::Result<std::fs::File> {
    use rustix::fs::OFlags;
    use std::os::unix::fs::OpenOptionsExt;
    let flags = match zone {
        Zone::Inside(_) => OFlags::NONBLOCK | OFlags::NOFOLLOW,
        Zone::Anywhere => OFlags::NONBLOCK,
    };
    let flags =
        i32::try_from(flags.bits()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
}

#[cfg(not(unix))]
fn open(path: &Path, _: Zone<'_>) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// `opened` is below `folder` (any spelling of it: prefixes, case where the system ignores it; a folder given in
/// another spelling than the system's own, such as an 8.3 name, compared once more in its canonical form).
fn below(opened: &Path, folder: &Path) -> bool {
    let inside = |folder: &Path| {
        crate::paths::strip_root(opened, folder).is_some_and(|rest| !rest.is_empty())
    };
    inside(folder) || crate::paths::canonicalize(folder).is_ok_and(|canon| inside(&canon))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone_dir() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let zone = crate::paths::canonicalize(dir.path())
            .unwrap()
            .join("zone ação");
        std::fs::create_dir(&zone).unwrap();
        (dir, zone)
    }

    fn refusal(result: Result<Vec<u8>, FsxError>) -> Option<io::ErrorKind> {
        match result {
            Err(FsxError::Io { source, .. }) => Some(source.kind()),
            _ => None,
        }
    }

    #[test]
    fn a_plain_file_reads_up_to_its_cap_in_and_out_of_a_zone() {
        let (_dir, zone) = zone_dir();
        let path = zone.join("a.json");
        std::fs::write(&path, b"{}").unwrap();
        for at in [Zone::Inside(&zone), Zone::Anywhere] {
            assert_eq!(read_regular(&path, 2, at).unwrap(), b"{}");
            assert!(matches!(
                read_regular(&path, 1, at),
                Err(FsxError::TooLarge { limit: 1, .. })
            ));
            assert_eq!(
                refusal(read_regular(&zone.join("missing"), 2, at)),
                Some(io::ErrorKind::NotFound)
            );
            assert_eq!(
                refusal(read_regular(&zone, 2, at)),
                Some(io::ErrorKind::InvalidData)
            );
        }
        // a zone named in another spelling than the system's own still holds its files
        let parent = zone.parent().unwrap();
        let spelled = zone.join("..").join("zone ação");
        assert_eq!(
            read_regular(&path, 2, Zone::Inside(&spelled)).unwrap(),
            b"{}"
        );
        // a file of another folder is not in this zone
        let other = parent.join("other.json");
        std::fs::write(&other, b"{}").unwrap();
        assert_eq!(
            refusal(read_regular(&other, 2, Zone::Inside(&zone))),
            Some(io::ErrorKind::InvalidData)
        );
    }

    /// A folder link (junction on Windows) on the way out of the zone: refused inside it, followed outside one.
    #[test]
    fn a_link_on_the_way_never_leads_a_zone_read_outside() {
        let (dir, zone) = zone_dir();
        let outside = crate::paths::canonicalize(dir.path())
            .unwrap()
            .join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("f.json"), b"{}").unwrap();
        let link = zone.join("sub");
        if !crate::tree::tests::link_out(&link, &outside) {
            eprintln!("skipped: no folder links here");
            return;
        }
        let path = link.join("f.json");
        assert_eq!(
            refusal(read_regular(&path, 2, Zone::Inside(&zone))),
            Some(io::ErrorKind::InvalidData)
        );
        assert_eq!(read_regular(&path, 2, Zone::Anywhere).unwrap(), b"{}");
        // the link itself is no plain file
        assert_eq!(
            refusal(read_regular(&link, 2, Zone::Inside(&zone))),
            Some(io::ErrorKind::InvalidData)
        );
    }

    /// A folder link deeper in a zone is refused before any name below it is looked up: following it could reach a
    /// network share (an outbound connection, a long wait). Seen here with a link to a folder that is gone: a lookup
    /// through it answers "not found", a refusal at the link "invalid data".
    #[test]
    fn a_link_in_the_middle_is_refused_before_anything_below_it_is_looked_up() {
        let (dir, zone) = zone_dir();
        let gone = crate::paths::canonicalize(dir.path()).unwrap().join("gone");
        std::fs::create_dir(&gone).unwrap();
        std::fs::create_dir(zone.join("plugins")).unwrap();
        let link = zone.join("plugins").join("git");
        if !crate::tree::tests::link_out(&link, &gone) {
            eprintln!("skipped: no folder links here");
            return;
        }
        std::fs::remove_dir(&gone).unwrap();
        for path in [link.join("f.json"), link.join("deeper").join("f.json")] {
            assert_eq!(
                refusal(read_regular(&path, 2, Zone::Inside(&zone))),
                Some(io::ErrorKind::InvalidData),
                "{}",
                path.display()
            );
        }
        // the zone's own folder is no link either
        let linked_zone = zone.join("linked zone");
        std::fs::create_dir(&gone).unwrap();
        std::fs::write(gone.join("f.json"), b"{}").unwrap();
        assert!(crate::tree::tests::link_out(&linked_zone, &gone));
        assert_eq!(
            refusal(read_regular(
                &linked_zone.join("f.json"),
                2,
                Zone::Inside(&linked_zone)
            )),
            Some(io::ErrorKind::InvalidData)
        );
    }

    /// A zone folder that is a link, named in another spelling than the system's own, is refused before that spelling
    /// is resolved: resolving it follows the link (a network share: an outbound connection, a long wait). Seen with a
    /// link to a folder that is gone: resolving it answers "not found", a refusal at the link "invalid data".
    #[test]
    fn a_linked_zone_folder_is_refused_before_its_spelling_is_resolved() {
        let (dir, zone) = zone_dir();
        let gone = crate::paths::canonicalize(dir.path()).unwrap().join("gone");
        std::fs::create_dir(&gone).unwrap();
        let linked = zone.join(".permark");
        if !crate::tree::tests::link_out(&linked, &gone) {
            eprintln!("skipped: no folder links here");
            return;
        }
        std::fs::remove_dir(&gone).unwrap();
        let spelled = zone.join("..").join("zone ação").join(".permark");
        assert_eq!(
            refusal(read_regular(
                &linked.join("f.json"),
                2,
                Zone::Inside(&spelled)
            )),
            Some(io::ErrorKind::InvalidData)
        );
    }

    /// What a zone walk takes for a link: a folder link (junction on Windows) and a file link, never a plain folder or
    /// file. A cloud placeholder (OneDrive Files On-Demand), which carries a reparse point that std reports as no link,
    /// cannot be made here: it walks like the plain folder.
    #[test]
    fn a_zone_walk_takes_only_what_std_reports_as_a_link() {
        let (_dir, zone) = zone_dir();
        let folder = zone.join("plain");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(zone.join("a.json"), b"{}").unwrap();
        let meta = |path: &Path| std::fs::symlink_metadata(path).unwrap();
        assert!(!is_link(&meta(&folder)));
        assert!(!is_link(&meta(&zone.join("a.json"))));
        let link = zone.join("link");
        if crate::tree::tests::link_out(&link, &folder) {
            assert!(is_link(&meta(&link)));
            assert!(!meta(&link).is_dir());
        }
        #[cfg(unix)]
        {
            let file_link = zone.join("file link");
            std::os::unix::fs::symlink(zone.join("a.json"), &file_link).unwrap();
            assert!(is_link(&meta(&file_link)));
        }
    }

    /// A file link is never followed in a zone; outside one it is (the profile); a FIFO is refused at once either way.
    #[cfg(unix)]
    #[test]
    fn a_file_link_and_a_fifo_are_refused_without_waiting() {
        let (dir, zone) = zone_dir();
        let target = dir.path().join("t.json");
        std::fs::write(&target, b"{}").unwrap();
        let link = zone.join("l.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            refusal(read_regular(&link, 2, Zone::Inside(&zone))),
            Some(io::ErrorKind::InvalidData)
        );
        assert_eq!(read_regular(&link, 2, Zone::Anywhere).unwrap(), b"{}");
        let fifo = zone.join("f.json");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        assert!(made.is_ok_and(|s| s.success()));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let inside = refusal(read_regular(&fifo, 2, Zone::Inside(&zone)));
            let anywhere = refusal(read_regular(&fifo, 2, Zone::Anywhere));
            let _ = tx.send((inside, anywhere));
        });
        let answers = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a read waited on a FIFO");
        assert_eq!(
            answers,
            (
                Some(io::ErrorKind::InvalidData),
                Some(io::ErrorKind::InvalidData)
            )
        );
    }
}
