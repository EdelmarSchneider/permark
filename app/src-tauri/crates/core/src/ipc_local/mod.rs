//! Local socket between PerMark processes of ONE user: single instance and the agent server of each project.
//!
//! Names are guessable and machine-wide, so neither side trusts the other by name:
//! - Windows: named pipe `\\.\pipe\permark-<id>`, owner-only DACL, first instance exclusive, remote clients
//!   rejected. Client opens with `SECURITY_IDENTIFICATION` (server cannot impersonate it) and checks the server
//!   process runs as this user; server checks each client the same way.
//! - Unix: socket `<id>.sock` in a private folder (0700, ours, not a link); listener holds `<id>.lock` (a second
//!   listener is refused instead of stealing the socket). Client refuses a folder that is not private; both sides
//!   check the peer's euid.
//!
//! Protocol: one JSON line each way per connection, line <= [`MAX_LINE`], the first line within
//! [`FIRST_LINE_DEADLINE`]; a server may hold a connection after its answer (a session, see [`Reply::Hold`]).

#[cfg(windows)]
mod identity;
#[cfg(windows)]
mod peek;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

    /// The agent endpoint of one project: per user + profile folder + project folder.
    ///
    /// # Errors
    /// Unix socket path too long.
    pub fn project(profile: &Path, root: &crate::paths::Root) -> Result<Self, IpcLocalError> {
        let user = user_name();
        let profile = crate::paths::fold(&crate::paths::slash(profile));
        let id = crate::fsx::hash(
            format!(
                "{user}
project
{profile}
{}",
                root.key().as_str()
            )
            .as_bytes(),
        )
        .hex();
        Self::in_folder(&default_folder(&user), &format!("p-{id}"))
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

/// How long a client may take to send its first line (a slow or silent peer is dropped).
pub const FIRST_LINE_DEADLINE: Duration = Duration::from_secs(2);
/// Connections open at once (being read, answered or held); more are dropped unread.
pub const MAX_CONNECTIONS: usize = 32;
/// Connections held after their answer ([`Reply::Hold`]).
pub const MAX_SESSIONS: usize = 8;
/// How often a held connection looks whether its peer left or the server stops.
const SESSION_POLL: Duration = Duration::from_millis(200);
/// Pause between reads of a first line that is still coming.
const READ_POLL: Duration = Duration::from_millis(5);

/// What the server does with a connection after its first line.
pub enum Reply<Resp> {
    /// Answer, then close.
    Answer(Resp),
    /// Answer, then keep the connection open until the peer leaves or the server stops; `guard` is dropped then
    /// (a session: the peer's presence is the point, nothing more is read).
    Hold(Resp, Box<dyn Send>),
}

/// One look at a stream that must not block.
enum Polled {
    Data(usize),
    Empty,
    Closed,
}

/// Prepares a stream for [`poll_read`] (Unix: nonblocking; Windows: the pipe is peeked instead).
#[allow(clippy::unnecessary_wraps)] // Windows: cannot fail; same signature on every OS
fn poll_mode(stream: &Stream, on: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        stream.set_nonblocking(on)
    }
    #[cfg(windows)]
    {
        let _ = (stream, on);
        Ok(())
    }
}

/// Reads what is there now, never waiting for more.
fn poll_read(stream: &Stream, buf: &mut [u8]) -> Polled {
    #[cfg(windows)]
    let buf = match peek::available(stream) {
        Ok(0) => return Polled::Empty,
        Ok(n) => {
            let n = usize::try_from(n).unwrap_or(usize::MAX).min(buf.len());
            &mut buf[..n]
        }
        Err(_) => return Polled::Closed,
    };
    let mut reader = stream;
    match reader.read(buf) {
        Ok(0) => Polled::Closed,
        Ok(n) => Polled::Data(n),
        Err(e)
            if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::Interrupted =>
        {
            Polled::Empty
        }
        Err(_) => Polled::Closed,
    }
}

/// Reads one JSON line that must be complete by `deadline` (polled without blocking: a peer that sends nothing,
/// or one byte at a time, cannot hold the thread longer).
///
/// # Errors
/// Too long, late, not JSON of `T`, closed early.
pub fn read_line_by<T: DeserializeOwned>(
    stream: &Stream,
    deadline: Instant,
) -> Result<T, IpcLocalError> {
    let io = |source| IpcLocalError::Io {
        place: "peer".to_owned(),
        source,
    };
    poll_mode(stream, true).map_err(io)?;
    let mut line: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 4096];
    let outcome = loop {
        match poll_read(stream, &mut chunk) {
            Polled::Closed => {
                break Err(IpcLocalError::BadMessage(
                    "closed before end of line".to_owned(),
                ))
            }
            Polled::Data(n) => {
                let part = &chunk[..n];
                if let Some(end) = part.iter().position(|b| *b == b'\n') {
                    line.extend_from_slice(&part[..=end]);
                    break Ok(());
                }
                line.extend_from_slice(part);
                if line.len() as u64 > MAX_LINE {
                    break Err(IpcLocalError::BadMessage("line too long".to_owned()));
                }
            }
            Polled::Empty => {
                if Instant::now() >= deadline {
                    break Err(IpcLocalError::BadMessage("too slow".to_owned()));
                }
                std::thread::sleep(READ_POLL);
            }
        }
    };
    outcome?;
    if line.len() as u64 > MAX_LINE {
        return Err(IpcLocalError::BadMessage("line too long".to_owned()));
    }
    poll_mode(stream, false).map_err(io)?;
    serde_json::from_slice(&line).map_err(|e| IpcLocalError::BadMessage(e.to_string()))
}

/// Opens a session: sends `hello`, reads the answer, and keeps the connection (dropping the stream ends it).
///
/// # Errors
/// `Unreachable`, `Untrusted`, `NoAnswer`, `BadMessage`.
pub fn open_session<Req: Serialize, Resp: DeserializeOwned + Send + 'static>(
    endpoint: &Endpoint,
    hello: &Req,
) -> Result<(Resp, Stream), IpcLocalError> {
    let mut stream = endpoint.connect()?;
    write_line(&mut stream, hello)?;
    let answer = read_line_by::<Resp>(&stream, Instant::now() + ANSWER_TIMEOUT)
        .map_err(|_| IpcLocalError::NoAnswer(endpoint.describe()))?;
    Ok((answer, stream))
}

/// Counts what is open; a slot is given back when its guard drops.
struct Slots {
    open: AtomicUsize,
    limit: usize,
}

struct Slot(Arc<Slots>);

impl Slots {
    fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            open: AtomicUsize::new(0),
            limit,
        })
    }

    fn take(self: &Arc<Self>) -> Option<Slot> {
        self.open
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.limit).then_some(n + 1)
            })
            .ok()
            .map(|_| Slot(Arc::clone(self)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Serves requests on a listening endpoint: one thread accepts, one short thread per connection answers (at most
/// [`MAX_CONNECTIONS`] at once, first line within [`FIRST_LINE_DEADLINE`]); held connections poll the stop flag.
/// Drop stops accepting (wakes the accept loop), ends held connections and joins the accept thread.
pub struct Server {
    stop: Arc<AtomicBool>,
    endpoint: Endpoint,
    thread: Option<JoinHandle<()>>,
    connections: Arc<Slots>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Answers each connection's line, then closes it.
    ///
    /// # Errors
    /// Thread spawn failure.
    pub fn spawn<Req, Resp, H>(listening: Listening, handler: H) -> io::Result<Self>
    where
        Req: DeserializeOwned + 'static,
        Resp: Serialize + 'static,
        H: Fn(Req) -> Resp + Send + Sync + 'static,
    {
        Self::spawn_with(listening, move |request| Reply::Answer(handler(request)))
    }

    /// Like [`Server::spawn`]; the handler may also keep a connection open ([`Reply::Hold`], at most
    /// [`MAX_SESSIONS`]: over it the answer is sent and the connection closed).
    ///
    /// # Errors
    /// Thread spawn failure.
    pub fn spawn_with<Req, Resp, H>(listening: Listening, handler: H) -> io::Result<Self>
    where
        Req: DeserializeOwned + 'static,
        Resp: Serialize + 'static,
        H: Fn(Req) -> Reply<Resp> + Send + Sync + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let endpoint = listening.endpoint.clone();
        let handler = Arc::new(handler);
        let flag = Arc::clone(&stop);
        let connections = Slots::new(MAX_CONNECTIONS);
        let sessions = Slots::new(MAX_SESSIONS);
        let slots = Arc::clone(&connections);
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
                    // a flood is dropped unread; the ones already open keep being served
                    let Some(slot) = slots.take() else {
                        continue;
                    };
                    let handler = Arc::clone(&handler);
                    let stop = Arc::clone(&flag);
                    let sessions = Arc::clone(&sessions);
                    let _ = std::thread::Builder::new()
                        .name("permark-ipc-conn".to_owned())
                        .spawn(move || {
                            let _slot = slot;
                            serve_one(&stream, &*handler, &stop, &sessions);
                        });
                }
            })?;
        Ok(Self {
            stop,
            endpoint,
            thread: Some(thread),
            connections,
        })
    }

    /// Connections open now (tests, status).
    #[must_use]
    pub fn open_connections(&self) -> usize {
        self.connections.open.load(Ordering::Acquire)
    }
}

fn serve_one<Req, Resp>(
    stream: &Stream,
    handler: &dyn Fn(Req) -> Reply<Resp>,
    stop: &AtomicBool,
    sessions: &Arc<Slots>,
) where
    Req: DeserializeOwned,
    Resp: Serialize,
{
    let Ok(request) = read_line_by::<Req>(stream, Instant::now() + FIRST_LINE_DEADLINE) else {
        return;
    };
    let mut writer = stream;
    match handler(request) {
        Reply::Answer(answer) => {
            let _ = write_line(&mut writer, &answer);
        }
        Reply::Hold(answer, guard) => {
            let session = sessions.take();
            if write_line(&mut writer, &answer).is_err() || session.is_none() {
                return;
            }
            hold(stream, stop);
            drop(guard);
        }
    }
}

/// Waits until the peer leaves (EOF | error) or the server stops; anything the peer sends is ignored.
fn hold(stream: &Stream, stop: &AtomicBool) {
    if poll_mode(stream, true).is_err() {
        return;
    }
    let mut sink = [0_u8; 256];
    while !stop.load(Ordering::Acquire) {
        match poll_read(stream, &mut sink) {
            Polled::Closed => return,
            Polled::Data(_) => {}
            Polled::Empty => std::thread::sleep(SESSION_POLL),
        }
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
        // held connections see the flag within one poll and let their guards go
        let deadline = Instant::now() + SESSION_POLL * 5;
        while self.open_connections() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests;
