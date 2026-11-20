//! Seals of PerMark's own action data: an HMAC-SHA256 under a key that never leaves this install (`install.key` in
//! the install folder), over a fixed list of framed fields, bound to the folder the data belongs to. Data that came
//! with a folder (a zip, a clone, a sync partner) holds no seal this install accepts, so PerMark never acts on it.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

use crate::fsx::{FolderIdentity, FsxError};
use crate::paths::Root;

const KEY_FILE: &str = "install.key";
const KEY_LEN: usize = 32;
/// Bytes of the HMAC a seal and a folder id keep.
const TAG_LEN: usize = 16;
const SEAL_PREFIX: &str = "s1:";
/// Starts every sealed message: the ids (`iid`, `rid`) start with other bytes, so no seal equals an id.
const SEAL_DOMAIN: &[u8] = b"permark/seal/1";
/// Waits before a short key file counts as broken: another process starting at the same moment may be writing it.
const SHORT_KEY_WAITS: [Duration; 5] = [Duration::from_millis(20); 5];

/// This install's key and the install id it gives.
#[derive(Debug)]
pub(super) struct Install {
    key: hmac::Key,
    iid: String,
}

impl Install {
    fn of(bytes: &[u8]) -> Self {
        let key = hmac::Key::new(hmac::HMAC_SHA256, bytes);
        let iid = hex(&hmac::sign(&key, b"iid").as_ref()[..8]);
        Self { key, iid }
    }
}

/// The key of the install folder `dir`, read (or made) once per folder in this process. `None`: no usable key (the
/// folder cannot be made, the key cannot be read or written): keep no sealed data.
pub(super) fn install(dir: &Path) -> Option<Arc<Install>> {
    static INSTALLS: OnceLock<Mutex<HashMap<PathBuf, Arc<Install>>>> = OnceLock::new();
    let mut installs = INSTALLS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = installs.get(dir) {
        return Some(Arc::clone(found));
    }
    let loaded = Arc::new(load(dir).ok()?);
    installs.insert(dir.to_path_buf(), Arc::clone(&loaded));
    Some(loaded)
}

/// What is at the key's path.
enum KeyFile {
    Missing,
    Key([u8; KEY_LEN]),
    /// Not a key this install keeps: wrong length, not a plain file (a link, a folder), open to other users.
    Bad,
}

/// Reads the key in `dir`, or makes one. A bad key is moved aside (never deleted) once, then a new one is made; a key
/// another process made meanwhile is read.
fn load(dir: &Path) -> io::Result<Install> {
    crate::fsx::private_dir(dir)?;
    let path = dir.join(KEY_FILE);
    let mut moved = false;
    for _ in 0..3 {
        match read_key(&path)? {
            KeyFile::Key(bytes) => return Ok(Install::of(&bytes)),
            KeyFile::Missing => match make_key(&path) {
                Ok(bytes) => return Ok(Install::of(&bytes)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            },
            KeyFile::Bad if !moved => {
                super::move_aside(&path)
                    .ok_or_else(|| io::Error::other("the install key could not be moved aside"))?;
                moved = true;
            }
            KeyFile::Bad => break,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "no usable install key",
    ))
}

fn read_key(path: &Path) -> io::Result<KeyFile> {
    let mut waits = SHORT_KEY_WAITS.iter();
    loop {
        let meta = match std::fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(KeyFile::Missing),
            Err(error) => return Err(error),
        };
        if !meta.file_type().is_file() || open_to_others(&meta) {
            return Ok(KeyFile::Bad);
        }
        let bytes = match crate::fsx::read_bytes(path, KEY_LEN as u64 + 1) {
            Ok(bytes) => bytes,
            Err(FsxError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(KeyFile::Missing)
            }
            Err(FsxError::Io { source, .. }) => return Err(source),
            Err(FsxError::TooLarge { .. } | FsxError::Binary(_)) => return Ok(KeyFile::Bad),
        };
        if let Ok(key) = <[u8; KEY_LEN]>::try_from(bytes.as_slice()) {
            return Ok(KeyFile::Key(key));
        }
        match waits.next() {
            Some(wait) if bytes.len() < KEY_LEN => std::thread::sleep(*wait),
            _ => return Ok(KeyFile::Bad),
        }
    }
}

/// A key file another user owns, or one group | others may read, may be known elsewhere: never used.
#[cfg(unix)]
#[allow(clippy::verbose_bit_mask)] // the group/other permission bits are the point
fn open_to_others(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o077 != 0
}

#[cfg(not(unix))]
fn open_to_others(_: &std::fs::Metadata) -> bool {
    false
}

/// A new key, created only where nothing is (a link counts): unix mode 0600 from the start.
fn make_key(path: &Path) -> io::Result<[u8; KEY_LEN]> {
    let mut key = [0; KEY_LEN];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| io::Error::other("no system randomness"))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(&key)?;
    file.sync_all()?;
    Ok(key)
}

/// This install's own data for one folder: the install id, the folder's id under this install's key, and the folder
/// that holds it (`<state dir>/machine/<iid>/`). The folder id is worked out again at each open, never stored, so a
/// file copied from another folder, another computer or planted names another id.
#[derive(Debug, Clone)]
pub struct Machine {
    key: hmac::Key,
    iid: String,
    rid: String,
    dir: PathBuf,
}

impl Machine {
    pub(super) fn new(install: &Install, root: &Root, state_dir: &Path) -> Self {
        Self {
            key: install.key.clone(),
            iid: install.iid.clone(),
            rid: rid(
                &install.key,
                FolderIdentity::of(root.path()),
                root.key().as_str(),
            ),
            dir: state_dir.join("machine").join(&install.iid),
        }
    }

    /// 16 hex characters; names this install's folder.
    #[must_use]
    pub fn iid(&self) -> &str {
        &self.iid
    }

    /// 32 hex characters; the same folder gives the same id under this install's key.
    #[must_use]
    pub fn rid(&self) -> &str {
        &self.rid
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A seal of `kind` data of this folder: the folder id is its first field.
    #[must_use]
    pub fn seal(&self, kind: &str) -> Seal {
        let mut seal = Seal::new(&self.key, kind);
        seal.field(self.rid.as_bytes());
        seal
    }
}

/// One seal: its kind, then each field in the order the kind lists them. A list = `num(count)`, then its elements.
/// Each field is framed (presence byte, u32 big-endian length, bytes), so two different field lists never seal alike.
#[derive(Debug, Clone)]
pub struct Seal {
    mac: hmac::Context,
}

impl Seal {
    fn new(key: &hmac::Key, kind: &str) -> Self {
        let mut mac = hmac::Context::with_key(key);
        mac.update(SEAL_DOMAIN);
        let mut seal = Self { mac };
        seal.field(kind.as_bytes());
        seal
    }

    pub fn field(&mut self, bytes: &[u8]) -> &mut Self {
        frame(&mut self.mac, Some(bytes));
        self
    }

    /// An optional field: absent is one presence byte, never equal to an empty field.
    pub fn opt(&mut self, bytes: Option<&[u8]>) -> &mut Self {
        frame(&mut self.mac, bytes);
        self
    }

    /// A number: its 8 bytes, big-endian.
    pub fn num(&mut self, n: u64) -> &mut Self {
        self.field(&n.to_be_bytes())
    }

    /// `s1:` + 32 hex characters.
    #[must_use]
    pub fn sign(&self) -> String {
        format!("{SEAL_PREFIX}{}", tag(self.mac.clone()))
    }

    /// `seal` is exactly the seal of these fields; compared in time that does not depend on where they differ.
    #[must_use]
    pub fn verify(&self, seal: &str) -> bool {
        same_bytes(self.sign().as_bytes(), seal.as_bytes())
    }
}

/// Presence byte, then (present) u32 big-endian length + bytes. Fields come from capped state files, far below 4 GiB.
fn frame(mac: &mut hmac::Context, bytes: Option<&[u8]>) {
    match bytes {
        None => mac.update(&[0]),
        Some(bytes) => {
            mac.update(&[1]);
            mac.update(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
            mac.update(bytes);
        }
    }
}

/// Id of a folder under `key`: its identity on disk (volume, file id, creation time where the platform keeps one),
/// else its path key.
fn rid(key: &hmac::Key, identity: Option<FolderIdentity>, root_key: &str) -> String {
    let mut mac = hmac::Context::with_key(key);
    mac.update(b"rid");
    if let Some(identity) = identity {
        frame(&mut mac, Some(b"folder"));
        frame(&mut mac, Some(&identity.volume.to_be_bytes()));
        frame(&mut mac, Some(&identity.file.to_be_bytes()));
        let born = identity.born.map(u64::to_be_bytes);
        frame(&mut mac, born.as_ref().map(<[u8; 8]>::as_slice));
    } else {
        frame(&mut mac, Some(b"key"));
        frame(&mut mac, Some(root_key.as_bytes()));
    }
    tag(mac)
}

/// Hex of the first [`TAG_LEN`] bytes of the HMAC.
fn tag(mac: hmac::Context) -> String {
    hex(&mac.sign().as_ref()[..TAG_LEN])
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Equal byte strings, in time that depends only on their lengths (`ring::constant_time` is deprecated, and
/// `hmac::verify` cannot check a shortened tag).
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .fold(0u8, |diff, (x, y)| std::hint::black_box(diff | (x ^ y)))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Key bytes 0, 1, ..., 31: the vectors below were computed from this key by an independent HMAC-SHA256.
    fn golden() -> Install {
        let bytes: Vec<u8> = (0..32).collect();
        Install::of(&bytes)
    }

    fn machine(rid: &str) -> Machine {
        let install = golden();
        Machine {
            key: install.key.clone(),
            iid: install.iid.clone(),
            rid: rid.to_owned(),
            dir: PathBuf::from("machine"),
        }
    }

    const RID: &str = "b8d4225c264a03c11909e7d6cbf095fa";

    #[test]
    fn seals_and_ids_match_the_golden_vectors() {
        let install = golden();
        assert_eq!(install.iid, "1f3e137ccf47fae5");
        assert_eq!(
            Seal::new(&install.key, "journal")
                .field(b"abc")
                .opt(None)
                .num(42)
                .opt(Some(b""))
                .sign(),
            "s1:189a76f10bf6d2404b1dc4e6519bf8b0"
        );
        assert_eq!(
            Seal::new(&install.key, "").sign(),
            "s1:cba4fe3e0a30ac0a294d9073ab314966"
        );
        let folder = |born| FolderIdentity {
            volume: 1,
            file: 2,
            born,
        };
        assert_eq!(rid(&install.key, Some(folder(Some(3))), "unused"), RID);
        assert_eq!(
            rid(&install.key, Some(folder(None)), "unused"),
            "19d29ec1cf3f1bfdda96e39ccb5d28c4"
        );
        assert_eq!(
            rid(&install.key, None, "c:/users/x/notes"),
            "231ce8f9bcdf92ade566da2e89c585ed"
        );
        let m = machine(RID);
        assert_eq!(
            m.seal("agent-context").sign(),
            "s1:a03e9044c40eede3da58848e1c8c4643"
        );
        // a list: its length, then its elements
        let mut list = m.seal("journal");
        list.num(2);
        for name in ["a.md", "b.md"] {
            list.field(name.as_bytes());
        }
        assert_eq!(list.sign(), "s1:1691b6c38f0457cb4527a0bed7b4d939");
    }

    #[test]
    fn framing_keeps_every_field_apart() {
        let key = golden().key;
        let seal = |kind: &str, fields: &[Option<&[u8]>]| {
            let mut seal = Seal::new(&key, kind);
            for field in fields {
                seal.opt(*field);
            }
            seal.sign()
        };
        let a: &[u8] = b"a";
        let ab: &[u8] = b"ab";
        let bc: &[u8] = b"bc";
        let c: &[u8] = b"c";
        assert_ne!(seal("ab", &[Some(c)]), seal("a", &[Some(bc)]));
        assert_ne!(
            seal("k", &[Some(ab), Some(c)]),
            seal("k", &[Some(a), Some(bc)])
        );
        assert_ne!(seal("k", &[None]), seal("k", &[Some(b"")]));
        assert_ne!(seal("k", &[None]), seal("k", &[]));
        assert_ne!(seal("k", &[Some(a)]), seal("j", &[Some(a)]));
    }

    #[test]
    fn verify_takes_only_the_exact_seal() {
        let m = machine(RID);
        let good = m.seal("journal").num(7).sign();
        assert_eq!(good.len(), 3 + 32);
        assert!(m.seal("journal").num(7).verify(&good));
        let flipped = format!(
            "{}{}",
            &good[..good.len() - 1],
            if good.ends_with('0') { '1' } else { '0' }
        );
        for bad in [
            flipped.as_str(),
            &good.to_uppercase(),
            &good[..good.len() - 2],
            &format!("{good}00"),
            &good[3..],
            "",
        ] {
            assert!(!m.seal("journal").num(7).verify(bad), "{bad}");
        }
        // another kind, another field, another folder, another key
        assert!(!m.seal("journal-x").num(7).verify(&good));
        assert!(!m.seal("journal").num(8).verify(&good));
        assert!(!machine("19d29ec1cf3f1bfdda96e39ccb5d28c4")
            .seal("journal")
            .num(7)
            .verify(&good));
        let other = Install::of(&[9; 32]);
        assert!(!Seal::new(&other.key, "journal")
            .field(RID.as_bytes())
            .num(7)
            .verify(&good));
    }

    fn broken(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().contains(".broken-"))
            .collect()
    }

    #[test]
    fn a_new_key_is_random_kept_and_read_back() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("install");
        let made = load(&dir).unwrap();
        let bytes = std::fs::read(dir.join(KEY_FILE)).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(made.iid, Install::of(&bytes).iid);
        assert_eq!(load(&dir).unwrap().iid, made.iid);
        assert_ne!(load(&base.path().join("other")).unwrap().iid, made.iid);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bits = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(bits(&dir), 0o700);
            assert_eq!(bits(&dir.join(KEY_FILE)), 0o600);
        }
    }

    #[test]
    fn a_bad_key_is_moved_aside_once_and_replaced() {
        for bad in [&b""[..], b"short", &[7; 40]] {
            let base = tempfile::tempdir().unwrap();
            let dir = base.path().join("install");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(KEY_FILE), bad).unwrap();
            let made = load(&dir).unwrap();
            let now = std::fs::read(dir.join(KEY_FILE)).unwrap();
            assert_eq!(now.len(), 32);
            assert_eq!(made.iid, Install::of(&now).iid);
            let aside = broken(&dir);
            assert_eq!(aside.len(), 1, "{bad:?}");
            assert_eq!(std::fs::read(&aside[0]).unwrap(), bad);
            // the new key is used from now on: no second move
            assert_eq!(load(&dir).unwrap().iid, made.iid);
            assert_eq!(broken(&dir).len(), 1);
        }
        // a folder in place of the key
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("install");
        std::fs::create_dir_all(dir.join(KEY_FILE)).unwrap();
        load(&dir).unwrap();
        assert!(dir.join(KEY_FILE).is_file());
        assert!(broken(&dir)[0].is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_key_others_can_read_or_a_link_is_moved_aside() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("install");
        let first = load(&dir).unwrap();
        let key = dir.join(KEY_FILE);
        let old = std::fs::read(&key).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        let next = load(&dir).unwrap();
        assert_ne!(next.iid, first.iid);
        assert_eq!(
            std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read(&broken(&dir)[0]).unwrap(), old);
        // a link in place of the key: the link itself is moved, what it points to stays as it is
        let elsewhere = base.path().join("elsewhere.key");
        std::fs::write(&elsewhere, [5; 32]).unwrap();
        std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::remove_file(&key).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &key).unwrap();
        let linked = load(&dir).unwrap();
        assert_ne!(linked.iid, Install::of(&[5; 32]).iid);
        assert!(!std::fs::symlink_metadata(&key)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(&elsewhere).unwrap(), [5; 32]);
    }

    #[test]
    fn a_key_still_being_written_is_waited_for() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("install");
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join(KEY_FILE);
        // made as another start makes it (0600 on unix), its bytes not written yet
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        drop(options.open(&key).unwrap());
        let writer = {
            let key = key.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                std::fs::write(&key, [3; 32]).unwrap();
            })
        };
        let read = load(&dir).unwrap();
        writer.join().unwrap();
        assert_eq!(read.iid, Install::of(&[3; 32]).iid);
        assert_eq!(broken(&dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn two_starts_at_once_share_one_key() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("install");
        let starts: Vec<_> = (0..4)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || load(&dir).unwrap().iid)
            })
            .collect();
        let ids: Vec<String> = starts.into_iter().map(|t| t.join().unwrap()).collect();
        assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
        assert_eq!(broken(&dir), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_key_is_read_once_per_install_folder() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("install");
        let first = install(&dir).unwrap();
        std::fs::remove_file(dir.join(KEY_FILE)).unwrap();
        let again = install(&dir).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(
            &first,
            &install(&base.path().join("b")).unwrap()
        ));
    }
}
