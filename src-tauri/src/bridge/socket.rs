//! The local socket of a project: where it is, how only its owner reaches it, and how both sides use it.
//!
//! The window of a project listens while the agent connection is on; `trailmark mcp` and `trailmark context`
//! connect. The name is derived from the project's folder and the user, so both sides find it without a registry:
//!
//! - **Windows**: a named pipe, `\\.\pipe\trailmark-<id>`. Its access list lets only the pipe's owner in, and the
//!   first instance is exclusive, so another program cannot take the name first.
//! - **Linux and macOS**: a Unix socket `<id>.sock` in a private folder (mode 0700) that is the user's runtime
//!   folder (`$XDG_RUNTIME_DIR`) or a per-user folder in the temporary folder. Nobody else can reach the socket
//!   through a folder they cannot enter; a folder that is not ours is refused. A listener holds a lock on
//!   `<id>.lock` beside the socket for as long as it lives, so a second window on the same project is refused
//!   instead of replacing the first one's socket, and a window removes only a socket that is its own.
//!
//! `<id>` is a hash of the project's key (the one that names its state folder) and the user's name.

use std::io;
use std::path::{Path, PathBuf};

use interprocess::local_socket::tokio::{Listener, Stream};
use interprocess::local_socket::traits::tokio::Listener as _;
#[cfg(unix)]
use interprocess::local_socket::traits::tokio::Stream as _;
use interprocess::local_socket::{ListenerOptions, Name};

use crate::util::hash_hex;

/// Unix socket paths are limited (104 bytes on macOS, 108 on Linux, with the terminator): a longer path is
/// refused with a message instead of an obscure error from the system.
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 100;

/// Why a socket cannot be made. Both reasons come from sockets that are files, so they are never made on Windows.
#[derive(Debug, thiserror::Error)]
#[cfg_attr(windows, allow(dead_code))]
pub(crate) enum SocketError {
    #[error(
        "the socket path is too long ({0}); set XDG_RUNTIME_DIR or TMPDIR to a shorter folder"
    )]
    PathTooLong(String),
    #[error("cannot prepare the private folder {path}: {source}")]
    Folder { path: String, source: io::Error },
}

/// Where a project's socket is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    /// What identifies it: the pipe name on Windows, the socket file elsewhere.
    #[cfg(windows)]
    pipe: String,
    #[cfg(unix)]
    path: PathBuf,
}

/// The user's name, reduced to what is safe in a file name.
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

/// Names the project's socket: the same for every program of this user that is given the same folder.
fn socket_id(project_root: &Path, user: &str) -> String {
    hash_hex(format!("{user}\n{}", crate::session::project_key(project_root)).as_bytes())
}

/// The user's private folder for sockets (see the module documentation).
#[cfg(unix)]
fn default_folder(user: &str) -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
    {
        return runtime.join("trailmark");
    }
    // macOS has no runtime folder; its temporary folder is already private to the user. Where it is shared (`/tmp`
    // when a Linux session sets no runtime folder), the user's name keeps the folders apart.
    std::env::temp_dir().join(format!("trailmark-{user}"))
}

/// Makes `dir` a folder only this user can enter. A folder that exists keeps its contents but gets the mode;
/// changing the mode of a folder that belongs to someone else fails, and so does a link in its place.
#[cfg(unix)]
fn make_private(dir: &Path) -> Result<(), SocketError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let fail = |source| SocketError::Folder {
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

impl Endpoint {
    /// The socket of the project in `project_root` (a canonical folder) for the current user.
    pub(crate) fn for_project(project_root: &Path) -> Result<Self, SocketError> {
        let user = user_name();
        #[cfg(unix)]
        let folder = default_folder(&user);
        #[cfg(windows)]
        let folder = PathBuf::new();
        Self::in_folder(&folder, project_root, &user)
    }

    /// Like [`Endpoint::for_project`] with the folder and the user's name given (the folder is ignored on
    /// Windows, where a pipe lives in its own namespace).
    // A pipe's name cannot be refused, so on Windows this always succeeds; the signature is the same on every system.
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn in_folder(
        folder: &Path,
        project_root: &Path,
        user: &str,
    ) -> Result<Self, SocketError> {
        let id = socket_id(project_root, user);
        #[cfg(windows)]
        {
            let _ = folder;
            Ok(Self {
                pipe: format!("trailmark-{id}"),
            })
        }
        #[cfg(unix)]
        {
            let path = folder.join(format!("{id}.sock"));
            if path.as_os_str().len() > MAX_SOCKET_PATH {
                return Err(SocketError::PathTooLong(path.display().to_string()));
            }
            Ok(Self { path })
        }
    }

    /// For messages and logs.
    pub(crate) fn describe(&self) -> String {
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
            use interprocess::local_socket::{GenericNamespaced, ToNsName};
            self.pipe.clone().to_ns_name::<GenericNamespaced>()
        }
        #[cfg(unix)]
        {
            use interprocess::local_socket::{GenericFilePath, ToFsName};
            self.path.clone().to_fs_name::<GenericFilePath>()
        }
    }

    /// Starts listening. Call it where a Tokio runtime is current. The name is exclusive: a second listener for
    /// the same project is refused while the first lives (the first named pipe of a name on Windows, the lock on
    /// `<id>.lock` elsewhere), and what a program that did not exit cleanly left behind is replaced.
    pub(crate) fn listen(&self) -> Result<Listening, ListenError> {
        let failed = |source| ListenError::Io {
            place: self.describe(),
            source,
        };
        let options = ListenerOptions::new().name(self.name().map_err(failed)?);
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::local_socket::ListenerOptionsExt;
            options.security_descriptor(owner_only().map_err(failed)?)
        };
        #[cfg(unix)]
        let (options, lock) = {
            let lock = self.take_lock()?;
            // Only this program's own leftover is in the way: the folder is private to the user, and the lock says
            // that no other listener of this project is alive.
            let options = options
                .try_overwrite(true)
                .max_spin_time(std::time::Duration::from_secs(1));
            (options, lock)
        };
        Ok(Listening {
            listener: options.create_tokio().map_err(failed)?,
            #[cfg(unix)]
            _lock: lock,
        })
    }

    /// Makes the private folder if needed and takes the lock that says this program is the project's listener.
    #[cfg(unix)]
    fn take_lock(&self) -> Result<std::fs::File, ListenError> {
        use std::fs::{OpenOptions, TryLockError};

        if let Some(folder) = self.path.parent() {
            make_private(folder)?;
        }
        let failed = |source| ListenError::Io {
            place: self.describe(),
            source,
        };
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("lock"))
            .map_err(failed)?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(TryLockError::WouldBlock) => Err(ListenError::InUse(self.describe())),
            Err(TryLockError::Error(source)) => Err(failed(source)),
        }
    }

    /// Connects to the window of the project, once it is clear that what listens is this user's: nothing is sent to a
    /// program that is not.
    ///
    /// The names are guessable and the socket can be taken while the window is closed, so a client does not assume
    /// that whoever answers is TrailMark. On Linux and macOS the folder of the socket must be a folder of the user's
    /// that nobody else can enter, and the program that listens must run as the user. On Windows the pipe is opened so
    /// that the server cannot act as the client, and the program that serves it must run as the user.
    pub(crate) async fn connect(&self) -> Result<Stream, ConnectError> {
        let place = self.describe();
        #[cfg(unix)]
        let stream = {
            self.check_folder()?;
            Stream::connect(self.name()?).await?
        };
        #[cfg(windows)]
        let stream = connect_pipe(&place).await?;
        check_peer(&stream, &place)?;
        Ok(stream)
    }

    /// The folder holding the socket is one only this user can use (see [`Endpoint::connect`]).
    #[cfg(unix)]
    fn check_folder(&self) -> Result<(), ConnectError> {
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
            Ok(_) => Err(ConnectError::Untrusted(format!(
                "{} is not a private folder of this user, so what listens in it is not trusted",
                folder.display()
            ))),
            // No folder, no socket: connecting says that nobody listens.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Why a client could not reach the window.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ConnectError {
    /// The connection could not be made (nothing listens, or the system refused).
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Something listens, but it is not shown to be this user's TrailMark, so nothing is sent to it.
    #[error("{0}")]
    Untrusted(String),
}

/// The user this program runs as.
#[cfg(unix)]
fn our_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Whether a folder is one only the user `ours` can use: a real folder (not a link) that is theirs and closed to
/// everybody else. `uid` and `mode` are the folder's owner and permissions.
#[cfg(unix)]
// The permission bits of the group and others (`rwxrwx` in octal) are what this tests; `trailing_zeros` would hide it.
#[allow(clippy::verbose_bit_mask)]
fn is_private_folder(is_dir: bool, uid: u32, mode: u32, ours: u32) -> bool {
    is_dir && uid == ours && mode & 0o077 == 0
}

/// Whether the program on the other end of the connection runs as the user `ours`.
#[cfg(unix)]
fn peer_is_us(peer: Option<u32>, ours: u32) -> bool {
    peer == Some(ours)
}

/// The program that listens runs as this user.
#[cfg(unix)]
fn check_peer(stream: &Stream, place: &str) -> Result<(), ConnectError> {
    use interprocess::local_socket::traits::StreamCommon as _;

    let creds = stream.peer_creds().map_err(|error| {
        ConnectError::Untrusted(format!("cannot tell who listens on {place}: {error}"))
    })?;
    if peer_is_us(creds.euid(), our_uid()) {
        Ok(())
    } else {
        Err(ConnectError::Untrusted(format!(
            "the program that listens on {place} does not run as this user"
        )))
    }
}

/// Opens the pipe at `path` so that the server can tell who connects but cannot act as them. At the default level
/// (`SECURITY_IMPERSONATION`) a server with the right privilege could call `ImpersonateNamedPipeClient` and use the
/// client's rights; `SECURITY_IDENTIFICATION` makes the token it gets good for nothing but looking.
#[cfg(windows)]
async fn connect_pipe(path: &str) -> Result<Stream, ConnectError> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::OwnedHandle;

    use interprocess::os::windows::named_pipe::local_socket::tokio::Stream as PipeStream;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, SECURITY_IDENTIFICATION};

    /// `ERROR_PIPE_BUSY`: every instance of the pipe has a client; the server makes a new one in a moment.
    const PIPE_BUSY: i32 = 231;
    const BUSY_TRIES: u32 = 100;
    const BUSY_PAUSE: std::time::Duration = std::time::Duration::from_millis(10);

    let mut tried = 0;
    let file = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(path)
        {
            Ok(file) => break file,
            Err(error) if error.raw_os_error() == Some(PIPE_BUSY) && tried < BUSY_TRIES => {
                tried += 1;
                tokio::time::sleep(BUSY_PAUSE).await;
            }
            Err(error) => return Err(error.into()),
        }
    };
    let pipe = PipeStream::try_from(OwnedHandle::from(file))
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(Stream::from(pipe))
}

/// The program that serves the pipe runs as this user (see [`identity`](super::identity)).
#[cfg(windows)]
fn check_peer(stream: &Stream, place: &str) -> Result<(), ConnectError> {
    use interprocess::local_socket::traits::StreamCommon as _;

    let untrusted = |why: String| ConnectError::Untrusted(format!("{place}: {why}"));
    let pid = stream
        .peer_creds()
        .map_err(|error| untrusted(format!("cannot tell who serves it: {error}")))?
        .pid()
        .ok_or_else(|| untrusted("cannot tell who serves it".to_owned()))?;
    match super::identity::runs_as_this_user(pid) {
        Ok(true) => Ok(()),
        Ok(false) => Err(untrusted(format!(
            "the program that serves it (process {pid}) runs as another user"
        ))),
        Err(error) => Err(untrusted(format!(
            "cannot tell who serves it (process {pid}): {error}"
        ))),
    }
}

/// A socket that is listening. Dropping it closes the socket and, on Unix, removes the socket file and then lets go of
/// the lock that kept other windows of the project away, in that order.
#[derive(Debug)]
pub(crate) struct Listening {
    listener: Listener,
    #[cfg(unix)]
    _lock: std::fs::File,
}

impl Listening {
    /// The next client.
    pub(crate) async fn accept(&self) -> io::Result<Stream> {
        self.listener.accept().await
    }
}

/// Why listening failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ListenError {
    #[error(transparent)]
    Socket(#[from] SocketError),
    #[error("cannot listen on {place}: {source}")]
    Io { place: String, source: io::Error },
    /// Another window of this user already listens for the project. Only made where sockets are files.
    #[error("another TrailMark window is already serving this project ({0})")]
    #[cfg_attr(windows, allow(dead_code))]
    InUse(String),
}

/// Access for the pipe's owner only (the owner-rights SID), whoever else the system would let in by default.
#[cfg(windows)]
fn owner_only() -> io::Result<interprocess::os::windows::security_descriptor::SecurityDescriptor> {
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let sddl = U16CString::from_str("D:P(A;;GA;;;OW)").map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&sddl)
}

/// A folder for the sockets of a test, short enough for the Unix limit on every system (a CI runner's temp folder is
/// long) and, as a real socket folder is, closed to everybody else.
#[cfg(test)]
pub(crate) fn socket_folder() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("tm")
        .tempdir_in(std::env::temp_dir())
        .expect("a folder for sockets");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("close the folder to others");
    }
    dir
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use interprocess::local_socket::traits::tokio::Stream as _;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::{timeout, Duration};

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    fn folder() -> tempfile::TempDir {
        socket_folder()
    }

    fn endpoint(dir: &tempfile::TempDir, project: &str) -> Endpoint {
        Endpoint::in_folder(dir.path(), Path::new(project), "tester").expect("endpoint")
    }

    #[test]
    fn the_same_project_and_user_share_a_socket_and_others_do_not() {
        let dir = folder();
        let a = endpoint(&dir, "/work/notes");
        assert_eq!(a, endpoint(&dir, "/work/notes"));
        assert_ne!(a, endpoint(&dir, "/work/other"));
        let other_user =
            Endpoint::in_folder(dir.path(), Path::new("/work/notes"), "someone").expect("endpoint");
        assert_ne!(
            a, other_user,
            "two users on one machine must not share a name"
        );
        // A trailing separator is the same folder.
        assert_eq!(a, endpoint(&dir, "/work/notes/"));
    }

    #[test]
    fn the_name_looks_like_the_documented_one() {
        let dir = folder();
        let described = endpoint(&dir, "/work/notes").describe();
        if cfg!(windows) {
            assert!(described.starts_with(r"\\.\pipe\trailmark-"), "{described}");
            assert_eq!(described.len(), r"\\.\pipe\trailmark-".len() + 16);
        } else {
            assert_eq!(
                Path::new(&described).extension().and_then(|e| e.to_str()),
                Some("sock"),
                "{described}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_path_too_long_for_a_socket_is_refused_with_a_reason() {
        let long = PathBuf::from(format!("/{}", "x".repeat(120)));
        let error = Endpoint::in_folder(&long, Path::new("/p"), "u").expect_err("too long");
        assert!(matches!(error, SocketError::PathTooLong(_)), "{error}");
    }

    #[tokio::test]
    async fn a_client_reaches_the_listener_and_lines_go_both_ways() {
        let dir = folder();
        let endpoint = endpoint(&dir, "/work/lines");
        let listener = endpoint.listen().expect("listen");
        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (recv, mut send) = stream.split();
            let mut lines = BufReader::new(recv);
            let mut line = String::new();
            lines.read_line(&mut line).await.expect("read");
            send.write_all(format!("echo {line}").as_bytes())
                .await
                .expect("write");
            send.flush().await.expect("flush");
        });

        let stream = timeout(WAIT, endpoint.connect())
            .await
            .expect("connected in time")
            .expect("connect");
        let (recv, mut send) = stream.split();
        send.write_all(b"hello\n").await.expect("write");
        send.flush().await.expect("flush");
        let mut reply = String::new();
        timeout(WAIT, BufReader::new(recv).read_line(&mut reply))
            .await
            .expect("answered in time")
            .expect("read");
        assert_eq!(reply, "echo hello\n");
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn nobody_answers_when_nothing_listens() {
        let dir = folder();
        let error = endpoint(&dir, "/work/nobody")
            .connect()
            .await
            .expect_err("nothing listens");
        assert!(
            matches!(
                &error,
                ConnectError::Io(e)
                    if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused)
            ),
            "{error:?}"
        );
    }

    #[cfg(unix)]
    mod unix_trust {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        #[test]
        fn a_folder_is_private_when_it_is_the_users_and_closed_to_others() {
            let ours = 1000;
            assert!(is_private_folder(true, ours, 0o040_700, ours));
            assert!(
                !is_private_folder(true, ours, 0o040_750, ours),
                "the group can enter"
            );
            assert!(
                !is_private_folder(true, ours, 0o040_705, ours),
                "others can enter"
            );
            assert!(
                !is_private_folder(true, 1001, 0o040_700, ours),
                "somebody else's"
            );
            assert!(
                !is_private_folder(true, 0, 0o040_700, ours),
                "root's is not the user's"
            );
            assert!(
                !is_private_folder(false, ours, 0o120_700, ours),
                "a link or a file"
            );
        }

        #[test]
        fn only_a_program_of_this_user_is_trusted() {
            assert!(peer_is_us(Some(1000), 1000));
            assert!(!peer_is_us(Some(1001), 1000));
            assert!(!peer_is_us(Some(0), 1000));
            assert!(!peer_is_us(None, 1000), "unknown is not trusted");
        }

        /// `/tmp/trailmark-<user>` is a name anybody can make first: a folder somebody else made, or one that is not
        /// closed, holds a socket that nothing is sent to.
        #[tokio::test]
        async fn a_folder_that_is_not_closed_to_others_is_not_trusted() {
            let dir = folder();
            let private = dir.path().join("sockets");
            let endpoint = Endpoint::in_folder(&private, Path::new("/work/loose"), "tester")
                .expect("endpoint");
            let _listener = endpoint.listen().expect("listen");
            timeout(WAIT, endpoint.connect())
                .await
                .expect("in time")
                .expect("a private folder is trusted");
            std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
            let error = timeout(WAIT, endpoint.connect())
                .await
                .expect("in time")
                .expect_err("not trusted");
            assert!(matches!(error, ConnectError::Untrusted(_)), "{error:?}");
        }

        #[tokio::test]
        async fn a_link_in_place_of_the_folder_is_not_trusted() {
            let dir = folder();
            let real = dir.path().join("real");
            let endpoint =
                Endpoint::in_folder(&real, Path::new("/work/linked"), "tester").expect("endpoint");
            let _listener = endpoint.listen().expect("listen");
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&real, &link).expect("symlink");
            let through_link =
                Endpoint::in_folder(&link, Path::new("/work/linked"), "tester").expect("endpoint");
            // the socket is there, behind the link
            assert!(link
                .join(through_link.path.file_name().expect("a name"))
                .exists());
            let error = timeout(WAIT, through_link.connect())
                .await
                .expect("in time")
                .expect_err("not trusted");
            assert!(matches!(error, ConnectError::Untrusted(_)), "{error:?}");
        }

        #[tokio::test]
        async fn a_folder_that_is_not_there_means_nobody_listens() {
            let dir = folder();
            let missing = dir.path().join("missing");
            let endpoint =
                Endpoint::in_folder(&missing, Path::new("/work/none"), "tester").expect("endpoint");
            let error = endpoint.connect().await.expect_err("nothing listens");
            assert!(matches!(error, ConnectError::Io(_)), "{error:?}");
        }
    }

    #[cfg(windows)]
    mod windows_trust {
        use std::io::{Read, Write};
        use std::thread::JoinHandle;

        use interprocess::os::windows::named_pipe::{pipe_mode, PipeListenerOptions};

        use super::*;

        /// `ERROR_BAD_IMPERSONATION_LEVEL`: what a server gets when it tries to use a client it may only identify.
        const BAD_IMPERSONATION_LEVEL: i32 = 1346;

        fn pipe_name(name: &str) -> String {
            format!(r"\\.\pipe\trailmark-test-{}-{name}", std::process::id())
        }

        /// A server that does what a squatter with the right privilege does: reads the client's first byte, acts as
        /// the client, and tries to open a file with the client's rights. It answers with the error it got, if any.
        fn impersonating_server(pipe: &str) -> JoinHandle<io::Result<Option<i32>>> {
            let listener = PipeListenerOptions::new()
                .path(Path::new(pipe))
                .create_duplex::<pipe_mode::Bytes>()
                .expect("a pipe");
            std::thread::spawn(move || {
                let mut connection = listener.accept()?;
                let mut byte = [0_u8; 1];
                connection.read_exact(&mut byte)?;
                let _acting_as_the_client = connection.impersonate_client()?;
                let program = std::env::current_exe()?;
                Ok(std::fs::File::open(program)
                    .err()
                    .and_then(|error| error.raw_os_error()))
            })
        }

        /// The point of the identification level: the server can see who the client is but cannot act as them.
        #[tokio::test]
        async fn the_server_cannot_act_as_the_client() {
            let pipe = pipe_name("identification");
            let server = impersonating_server(&pipe);
            let stream = timeout(WAIT, connect_pipe(&pipe))
                .await
                .expect("in time")
                .expect("connected");
            let (_receive, mut send) = stream.split();
            send.write_all(&[1]).await.expect("write");
            assert_eq!(
                server.join().expect("server thread").expect("server"),
                Some(BAD_IMPERSONATION_LEVEL)
            );
        }

        /// What it would be without that: opening the pipe the plain way lets the same server use the client's rights.
        #[test]
        fn a_plainly_opened_pipe_does_let_the_server_act_as_the_client() {
            let pipe = pipe_name("plain");
            let server = impersonating_server(&pipe);
            let mut client = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pipe)
                .expect("connected");
            client.write_all(&[1]).expect("write");
            assert_eq!(server.join().expect("server thread").expect("server"), None);
        }

        #[tokio::test]
        async fn a_server_of_this_user_is_trusted() {
            let dir = folder();
            let endpoint = endpoint(&dir, "/work/trusted");
            let _listener = endpoint.listen().expect("listen");
            timeout(WAIT, endpoint.connect())
                .await
                .expect("in time")
                .expect("this user's server is trusted");
        }
    }

    #[tokio::test]
    async fn the_name_is_free_again_once_the_listener_is_dropped() {
        let dir = folder();
        let endpoint = endpoint(&dir, "/work/dropped");
        drop(endpoint.listen().expect("first"));
        drop(endpoint.listen().expect("second"));
    }

    /// Two windows on one project (opened twice, or two starts at once): the first keeps the socket, the second is told.
    #[tokio::test]
    async fn a_second_listener_does_not_take_the_name_of_the_first() {
        let dir = folder();
        let endpoint = endpoint(&dir, "/work/twice");
        let first = endpoint.listen().expect("first");
        assert!(endpoint.listen().is_err(), "the name is taken");
        // the refused attempt left the first listener where it was
        let server = tokio::spawn(async move {
            let stream = first.accept().await.expect("accept");
            drop(stream);
            first
        });
        timeout(WAIT, endpoint.connect())
            .await
            .expect("connected in time")
            .expect("the first window still answers");
        let first = server.await.expect("server task");
        // and once the first is gone the name is free for a new one
        drop(first);
        drop(endpoint.listen().expect("free again"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn closing_a_listener_removes_its_own_socket_only() {
        let dir = folder();
        let endpoint = endpoint(&dir, "/work/own");
        let first = endpoint.listen().expect("first");
        let _ = endpoint.listen().expect_err("refused");
        assert!(
            endpoint.path.exists(),
            "the refused attempt did not remove it"
        );
        drop(first);
        assert!(
            !endpoint.path.exists(),
            "the owner removes it when it closes"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_socket_lives_in_a_folder_only_its_owner_can_enter() {
        use std::os::unix::fs::PermissionsExt;

        let dir = folder();
        let private = dir.path().join("sockets");
        // Even when the folder exists with looser permissions, it is tightened.
        std::fs::create_dir(&private).expect("mkdir");
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let endpoint =
            Endpoint::in_folder(&private, Path::new("/work/notes"), "tester").expect("endpoint");
        let _listener = endpoint.listen().expect("listen");
        let mode = std::fs::metadata(&private)
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_socket_file_left_behind_does_not_stop_a_new_listener() {
        let dir = folder();
        let endpoint = endpoint(&dir, "/work/leftover");
        let path = endpoint.path.clone();
        std::fs::write(&path, b"leftover").expect("write");
        let _listener = endpoint.listen().expect("listen over the leftover");
        let stream = timeout(WAIT, endpoint.connect()).await.expect("in time");
        assert!(stream.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_in_place_of_the_private_folder_is_refused() {
        let dir = folder();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("mkdir");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let endpoint =
            Endpoint::in_folder(&link, Path::new("/work/notes"), "tester").expect("endpoint");
        assert!(matches!(
            endpoint.listen().expect_err("refused"),
            ListenError::Socket(SocketError::Folder { .. })
        ));
    }
}
