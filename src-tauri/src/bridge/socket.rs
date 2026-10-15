//! The local socket of a project: where it is, how only its owner reaches it, and how both sides use it.
//!
//! The window of a project listens while the agent connection is on; `trailmark mcp` and `trailmark context`
//! connect. The name is derived from the project's folder and the user, so both sides find it without a registry:
//!
//! - **Windows**: a named pipe, `\\.\pipe\trailmark-<id>`. Its access list lets only the pipe's owner in, and the
//!   first instance is exclusive, so another program cannot take the name first.
//! - **Linux and macOS**: a Unix socket `<id>.sock` in a private folder (mode 0700) that is the user's runtime
//!   folder (`$XDG_RUNTIME_DIR`) or a per-user folder in the temporary folder. Nobody else can reach the socket
//!   through a folder they cannot enter; a folder that is not ours is refused.
//!
//! `<id>` is a hash of the project's key (the one that names its state folder) and the user's name.

use std::io;
use std::path::{Path, PathBuf};

use interprocess::local_socket::tokio::{Listener, Stream};
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

    /// Starts listening. Call it where a Tokio runtime is current. A socket file left by a program that did
    /// not exit cleanly is replaced; the first named pipe of a name is exclusive.
    pub(crate) fn listen(&self) -> Result<Listener, ListenError> {
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
        let options = {
            if let Some(folder) = self.path.parent() {
                make_private(folder)?;
            }
            // Only this program's own leftover is in the way: the folder is private to the user.
            options
                .try_overwrite(true)
                .max_spin_time(std::time::Duration::from_secs(1))
        };
        options.create_tokio().map_err(failed)
    }

    /// Connects to the window of the project.
    pub(crate) async fn connect(&self) -> io::Result<Stream> {
        Stream::connect(self.name()?).await
    }
}

/// Why listening failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ListenError {
    #[error(transparent)]
    Socket(#[from] SocketError),
    #[error("cannot listen on {place}: {source}")]
    Io { place: String, source: io::Error },
}

/// Access for the pipe's owner only (the owner-rights SID), whoever else the system would let in by default.
#[cfg(windows)]
fn owner_only() -> io::Result<interprocess::os::windows::security_descriptor::SecurityDescriptor> {
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let sddl = U16CString::from_str("D:P(A;;GA;;;OW)").map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&sddl)
}

#[cfg(test)]
mod tests {
    use interprocess::local_socket::traits::tokio::Listener as _;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::{timeout, Duration};

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    /// A folder for sockets short enough for the Unix limit on every system (a CI runner's temp folder is long).
    fn folder() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("tm")
            .tempdir_in(std::env::temp_dir())
            .expect("tempdir")
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
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn the_name_is_free_again_once_the_listener_is_dropped() {
        let dir = folder();
        let endpoint = endpoint(&dir, "/work/dropped");
        drop(endpoint.listen().expect("first"));
        drop(endpoint.listen().expect("second"));
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
