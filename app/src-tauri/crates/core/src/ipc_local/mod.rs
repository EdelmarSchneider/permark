//! Local socket between PerMark processes of ONE user (single instance now; agent server later).
//!
//! Names are guessable and machine-wide, so neither side trusts the other by name:
//! - Windows: named pipe `\\.\pipe\permark-<id>`, owner-only DACL, first instance exclusive, remote clients
//!   rejected. Client opens with `SECURITY_IDENTIFICATION` (server cannot impersonate it) and checks the server
//!   process runs as this user; server checks each client the same way.
//! - Unix: socket `<id>.sock` in a private folder (0700, ours, not a link); listener holds `<id>.lock` (a second
//!   listener is refused instead of stealing the socket). Client refuses a folder that is not private; both sides
//!   check the peer's euid.
//!
//! Protocol: one JSON line each way per connection, line <= [`MAX_LINE`].

#[cfg(windows)]
mod identity;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name, Stream};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::state::AppDirs;

/// Longest accepted line (bytes).
pub const MAX_LINE: u64 = 64 * 1024;
/// How long a client waits for the answer.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum IpcLocalError {
    #[error("socket path too long ({0}); set XDG_RUNTIME_DIR or TMPDIR to a shorter folder")]
    PathTooLong(String),
    #[error("cannot prepare private folder {path}: {source}")]
    Folder { path: String, source: io::Error },
    #[error("another PerMark already listens on {0}")]
    InUse(String),
    /// Nothing listens (or the system refused the connection).
    #[error("cannot reach {place}: {source}")]
    Unreachable { place: String, source: io::Error },
    /// Something listens but is not shown to be this user's PerMark: nothing sent.
    #[error("untrusted peer on {0}")]
    Untrusted(String),
    #[error("no answer from {0}")]
    NoAnswer(String),
    #[error("bad message: {0}")]
    BadMessage(String),
    #[error("{place}: {source}")]
    Io { place: String, source: io::Error },
}

/// User name reduced to file-name-safe chars (<= 32).
fn user_name() -> String {
    let raw = ["USERNAME", "USER", "LOGNAME"]
        .into_iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "user".to_owned());
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect()
}

/// Where one service of this user listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    #[cfg(windows)]
    pipe: String,
    #[cfg(unix)]
    path: PathBuf,
}

/// Unix socket paths are limited (104 macOS, 108 Linux incl. NUL).
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 100;

impl Endpoint {
    /// The app's single-instance endpoint: per user + profile dir (a debug build has its own profile -> own name).
    ///
    /// # Errors
    /// Unix socket path too long.
    pub fn app(dirs: &AppDirs) -> Result<Self, IpcLocalError> {
        let user = user_name();
        let profile = crate::paths::fold(&crate::paths::slash(&dirs.profile));
        let id = crate::fsx::hash(format!("{user}\napp\n{profile}").as_bytes()).hex();
        Self::in_folder(&default_folder(&user), &format!("app-{id}"))
    }

    /// Endpoint `<id>` in `folder` (Unix: socket file there; Windows: pipe namespace, folder unused).
    ///
    /// # Errors
    /// Unix socket path too long.
    #[allow(clippy::unnecessary_wraps)] // Windows: cannot fail; same signature on every OS
    pub fn in_folder(folder: &Path, id: &str) -> Result<Self, IpcLocalError> {
        #[cfg(windows)]
        {
            let _ = folder;
            Ok(Self {
                pipe: format!("permark-{id}"),
            })
        }
        #[cfg(unix)]
        {
            let path = folder.join(format!("{id}.sock"));
            if path.as_os_str().len() > MAX_SOCKET_PATH {
                return Err(IpcLocalError::PathTooLong(path.display().to_string()));
            }
            Ok(Self { path })
        }
    }

    #[must_use]
    pub fn describe(&self) -> String {
        #[cfg(windows)]
        {
            format!(r"\\.\pipe\{}", self.pipe)
        }
        #[cfg(unix)]
        {
            self.path.display().to_string()
        }
    }

    fn name(&self) -> io::Result<Name<'static>> {
        #[cfg(windows)]
        {
            use interprocess::local_socket::GenericNamespaced;
            self.pipe.clone().to_ns_name::<GenericNamespaced>()
        }
        #[cfg(unix)]
        {
            use interprocess::local_socket::GenericFilePath;
            self.path.clone().to_fs_name::<GenericFilePath>()
        }
    }

    fn io(&self, source: io::Error) -> IpcLocalError {
        IpcLocalError::Io {
            place: self.describe(),
            source,
        }
    }

    /// Starts listening (exclusive per endpoint).
    ///
    /// # Errors
    /// `InUse` (another listener alive), `Folder`, `Io`.
    pub fn listen(&self) -> Result<Listening, IpcLocalError> {
        let options = ListenerOptions::new().name(self.name().map_err(|e| self.io(e))?);
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::local_socket::ListenerOptionsExt;
            options.security_descriptor(owner_only().map_err(|e| self.io(e))?)
        };
        #[cfg(unix)]
        let (options, lock) = {
            let lock = self.take_lock()?;
            // only our own leftover can be there: private folder + lock held
            (options.try_overwrite(true), lock)
        };
        let listener = options.create_sync().map_err(|e| {
            if e.kind() == io::ErrorKind::AddrInUse
                || e.raw_os_error() == Some(231)
                || e.raw_os_error() == Some(5)
            {
                IpcLocalError::InUse(self.describe())
            } else {
                self.io(e)
            }
        })?;
        Ok(Listening {
            listener,
            endpoint: self.clone(),
            #[cfg(unix)]
            _lock: lock,
        })
    }

    #[cfg(unix)]
    fn take_lock(&self) -> Result<std::fs::File, IpcLocalError> {
        use std::fs::{OpenOptions, TryLockError};
        if let Some(folder) = self.path.parent() {
            make_private(folder)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("lock"))
            .map_err(|e| self.io(e))?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(TryLockError::WouldBlock) => Err(IpcLocalError::InUse(self.describe())),
            Err(TryLockError::Error(e)) => Err(self.io(e)),
        }
    }

    /// Connects once the listener is shown to be this user's.
    ///
    /// # Errors
    /// `Unreachable` (nothing listens), `Untrusted`.
    pub fn connect(&self) -> Result<Stream, IpcLocalError> {
        let unreachable = |source| IpcLocalError::Unreachable {
            place: self.describe(),
            source,
        };
        #[cfg(unix)]
        let stream = {
            self.check_folder()?;
            Stream::connect(self.name().map_err(unreachable)?).map_err(unreachable)?
        };
        #[cfg(windows)]
        let stream = connect_pipe(&self.describe()).map_err(unreachable)?;
        check_peer(&stream, &self.describe())?;
        Ok(stream)
    }

    #[cfg(unix)]
    fn check_folder(&self) -> Result<(), IpcLocalError> {
        use std::os::unix::fs::MetadataExt;
        let Some(folder) = self.path.parent() else {
            return Ok(());
        };
        match std::fs::symlink_metadata(folder) {
            Ok(meta)
                if is_private_folder(
                    meta.file_type().is_dir(),
                    meta.uid(),
                    meta.mode(),
                    our_uid(),
                ) =>
            {
                Ok(())
            }
            Ok(_) => Err(IpcLocalError::Untrusted(format!(
                "{} is not a private folder",
                folder.display()
            ))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(self.io(e)),
        }
    }
}

/// Default private folder for sockets.
#[cfg(unix)]
fn default_folder(user: &str) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| {
                Some(PathBuf::from(format!("/run/user/{}", our_uid()))).filter(|d| d.is_dir())
            });
        if let Some(runtime) = runtime {
            return runtime.join("permark");
        }
    }
    let temp = std::env::temp_dir();
    if cfg!(target_os = "macos") && temp != Path::new("/tmp") {
        temp.join("permark") // macOS temp is per user already
    } else {
        temp.join(format!("permark-{user}"))
    }
}

#[cfg(windows)]
fn default_folder(_: &str) -> PathBuf {
    PathBuf::new()
}

#[cfg(unix)]
fn our_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Real folder, ours, closed to group + others.
#[cfg(unix)]
#[allow(clippy::verbose_bit_mask)] // the group/other permission bits are the point
fn is_private_folder(is_dir: bool, uid: u32, mode: u32, ours: u32) -> bool {
    is_dir && uid == ours && mode & 0o077 == 0
}

/// Makes `dir` a folder only we can enter (existing: mode reset; someone else's | a link -> error).
#[cfg(unix)]
fn make_private(dir: &Path) -> Result<(), IpcLocalError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let fail = |source| IpcLocalError::Folder {
        path: dir.display().to_string(),
        source,
    };
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(fail)?;
    let meta = std::fs::symlink_metadata(dir).map_err(fail)?;
    if !meta.file_type().is_dir() {
        return Err(fail(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a folder",
        )));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(fail)
}

#[cfg(unix)]
fn check_peer(stream: &Stream, place: &str) -> Result<(), IpcLocalError> {
    let creds = stream
        .peer_creds()
        .map_err(|e| IpcLocalError::Untrusted(format!("{place}: cannot tell who is there: {e}")))?;
    if creds.euid() == Some(our_uid()) {
        Ok(())
    } else {
        Err(IpcLocalError::Untrusted(format!(
            "{place}: peer runs as another user"
        )))
    }
}

#[cfg(windows)]
fn check_peer(stream: &Stream, place: &str) -> Result<(), IpcLocalError> {
    let untrusted = |why: String| IpcLocalError::Untrusted(format!("{place}: {why}"));
    let pid = stream
        .peer_creds()
        .map_err(|e| untrusted(format!("cannot tell who is there: {e}")))?
        .pid()
        .ok_or_else(|| untrusted("cannot tell who is there".to_owned()))?;
    match identity::runs_as_this_user(pid) {
        Ok(true) => Ok(()),
        Ok(false) => Err(untrusted(format!("process {pid} runs as another user"))),
        Err(e) => Err(untrusted(format!("cannot check process {pid}: {e}"))),
    }
}

/// Opens the pipe so the server can identify but never impersonate us; retries while all instances are busy.
#[cfg(windows)]
fn connect_pipe(path: &str) -> io::Result<Stream> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::OwnedHandle;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, SECURITY_IDENTIFICATION};
    const PIPE_BUSY: i32 = 231;
    let mut tries = 0;
    let file = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(path)
        {
            Ok(file) => break file,
            Err(e) if e.raw_os_error() == Some(PIPE_BUSY) && tries < 100 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    };
    let pipe = interprocess::os::windows::named_pipe::local_socket::Stream::try_from(
        OwnedHandle::from(file),
    )
    .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(Stream::from(pipe))
}

/// Owner-rights only (whoever else the system would let in by default).
#[cfg(windows)]
fn owner_only() -> io::Result<interprocess::os::windows::security_descriptor::SecurityDescriptor> {
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)").map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&sddl)
}

/// A listening endpoint. Drop closes it (Unix: socket file stays until the next listener replaces it; lock released).
#[derive(Debug)]
pub struct Listening {
    listener: interprocess::local_socket::Listener,
    endpoint: Endpoint,
    #[cfg(unix)]
    _lock: std::fs::File,
}

impl Listening {
    /// Next client that is this user's (untrusted ones dropped unanswered -> `Ok(None)`).
    ///
    /// # Errors
    /// Accept failure.
    pub fn accept_trusted(&self) -> io::Result<Option<Stream>> {
        let stream = self.listener.accept()?;
        Ok(check_peer(&stream, &self.endpoint.describe())
            .ok()
            .map(|()| stream))
    }
}

/// Writes one JSON line.
///
/// # Errors
/// Encode | IO failure.
pub fn write_line<T: Serialize>(stream: &mut impl Write, value: &T) -> Result<(), IpcLocalError> {
    let mut line =
        serde_json::to_vec(value).map_err(|e| IpcLocalError::BadMessage(e.to_string()))?;
    line.push(b'\n');
    stream
        .write_all(&line)
        .and_then(|()| stream.flush())
        .map_err(|source| IpcLocalError::Io {
            place: "peer".to_owned(),
            source,
        })
}

/// Reads one JSON line of at most [`MAX_LINE`] bytes.
///
/// # Errors
/// Too long, not JSON of `T`, closed early.
pub fn read_line<T: DeserializeOwned>(reader: &mut impl BufRead) -> Result<T, IpcLocalError> {
    let mut line = Vec::new();
    let read = reader
        .take(MAX_LINE + 1)
        .read_until(b'\n', &mut line)
        .map_err(|source| IpcLocalError::Io {
            place: "peer".to_owned(),
            source,
        })?;
    if read as u64 > MAX_LINE {
        return Err(IpcLocalError::BadMessage("line too long".to_owned()));
    }
    if line.last() != Some(&b'\n') {
        return Err(IpcLocalError::BadMessage(
            "closed before end of line".to_owned(),
        ));
    }
    serde_json::from_slice(&line).map_err(|e| IpcLocalError::BadMessage(e.to_string()))
}

/// Sends `request` to `endpoint`, waits for the answer (bounded by [`ANSWER_TIMEOUT`]).
///
/// # Errors
/// `Unreachable` (nothing listens), `Untrusted`, `NoAnswer`, `BadMessage`.
pub fn request<Req: Serialize, Resp: DeserializeOwned + Send + 'static>(
    endpoint: &Endpoint,
    request: &Req,
) -> Result<Resp, IpcLocalError> {
    let mut stream = endpoint.connect()?;
    write_line(&mut stream, request)?;
    let (tx, rx) = mpsc::channel();
    // a peer that never answers must not hang the caller: read on a helper thread
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let _ = tx.send(read_line::<Resp>(&mut reader));
    });
    rx.recv_timeout(ANSWER_TIMEOUT)
        .map_err(|_| IpcLocalError::NoAnswer(endpoint.describe()))?
}

/// Serves requests on a listening endpoint: one thread accepts, one short thread per connection answers.
/// Drop stops accepting (wakes the accept loop) and joins it (teardown symmetry).
pub struct Server {
    stop: Arc<AtomicBool>,
    endpoint: Endpoint,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// # Errors
    /// Thread spawn failure.
    pub fn spawn<Req, Resp, H>(listening: Listening, handler: H) -> io::Result<Self>
    where
        Req: DeserializeOwned + 'static,
        Resp: Serialize + 'static,
        H: Fn(Req) -> Resp + Send + Sync + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let endpoint = listening.endpoint.clone();
        let handler = Arc::new(handler);
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("permark-ipc".to_owned())
            .spawn(move || {
                while !flag.load(Ordering::Acquire) {
                    let stream = match listening.accept_trusted() {
                        Ok(Some(stream)) => stream,
                        Ok(None) => continue,
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(50));
                            continue;
                        }
                    };
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    let handler = Arc::clone(&handler);
                    let _ = std::thread::Builder::new()
                        .name("permark-ipc-conn".to_owned())
                        .spawn(move || {
                            let mut reader = BufReader::new(stream);
                            if let Ok(request) = read_line::<Req>(&mut reader) {
                                let answer = handler(request);
                                let _ = write_line(reader.get_mut(), &answer);
                            }
                        });
                }
            })?;
        Ok(Self {
            stop,
            endpoint,
            thread: Some(thread),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // wake the blocking accept; if that fails the thread is left (never hang shutdown)
        let woke = self.endpoint.name().and_then(Stream::connect).is_ok();
        if let Some(thread) = self.thread.take() {
            if woke {
                let _ = thread.join();
            }
        }
    }
}

#[cfg(test)]
mod tests;
