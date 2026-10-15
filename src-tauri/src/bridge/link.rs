//! The other side of the local socket: how `trailmark mcp` and `trailmark context` ask the window of a project.
//!
//! One connection is kept open for as long as the program lives, so the window can tell that an agent is connected.
//! A connection that went stale (the window was closed and opened again, or the connection was turned off and on)
//! is replaced by a new one without the caller noticing.

use std::io;
use std::time::Duration;

use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
use interprocess::local_socket::traits::tokio::Stream as _;
use serde_json::Value;
use tokio::io::BufReader;
use tokio::sync::Mutex;
use tokio::time::timeout;

use super::protocol::{
    read_line, write_line, Call, ProtocolError, Reply, Request, MAX_REPLY_BYTES,
};
use super::socket::{ConnectError, Endpoint};

/// How long to wait for the window to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long to wait for an answer. A search of a big project is the slowest thing the window does.
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// Nothing listens: the window is closed, or the connection is off.
    #[error("TrailMark is not open on this project")]
    NotRunning,
    /// The window answered with an error.
    #[error("{}", .0.message)]
    Remote(ProtocolError),
    #[error("the connection to TrailMark failed: {0}")]
    Io(#[from] io::Error),
    #[error("TrailMark did not answer in time")]
    TimedOut,
    /// Something listens where TrailMark would, but it is not shown to be this user's TrailMark: nothing was sent to it.
    #[error("not trusted, so nothing was sent: {0}")]
    Untrusted(String),
}

struct Connection {
    reader: BufReader<RecvHalf>,
    writer: SendHalf,
}

struct Inner {
    connection: Option<Connection>,
    next_id: u64,
}

/// A connection to the window of one project.
pub struct Link {
    endpoint: Endpoint,
    inner: Mutex<Inner>,
}

impl Link {
    pub(crate) fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            inner: Mutex::new(Inner {
                connection: None,
                next_id: 1,
            }),
        }
    }

    /// Asks the window and returns its result.
    pub async fn call(&self, request: Request) -> Result<Value, LinkError> {
        let mut inner = self.inner.lock().await;
        let id = inner.next_id;
        inner.next_id += 1;
        let call = Call { id, request };
        // A connection that was open may have gone stale; a new one gets a second chance, a fresh one does not.
        let mut reused = inner.connection.is_some();
        loop {
            if inner.connection.is_none() {
                inner.connection = Some(self.connect().await?);
                reused = false;
            }
            let Some(connection) = inner.connection.as_mut() else {
                return Err(LinkError::NotRunning);
            };
            match timeout(CALL_TIMEOUT, exchange(connection, &call)).await {
                Ok(Ok(reply)) => return reply.into_result().map_err(LinkError::Remote),
                Ok(Err(error)) => {
                    inner.connection = None;
                    if !reused {
                        return Err(LinkError::Io(error));
                    }
                    reused = false;
                }
                Err(_) => {
                    inner.connection = None;
                    return Err(LinkError::TimedOut);
                }
            }
        }
    }

    async fn connect(&self) -> Result<Connection, LinkError> {
        let stream = match timeout(CONNECT_TIMEOUT, self.endpoint.connect()).await {
            Err(_) => return Err(LinkError::TimedOut),
            Ok(Err(ConnectError::Io(error)))
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Err(LinkError::NotRunning)
            }
            Ok(Err(ConnectError::Io(error))) => return Err(LinkError::Io(error)),
            Ok(Err(ConnectError::Untrusted(why))) => return Err(LinkError::Untrusted(why)),
            Ok(Ok(stream)) => stream,
        };
        let (recv, writer) = stream.split();
        Ok(Connection {
            reader: BufReader::new(recv),
            writer,
        })
    }

    /// Connects now if there is a window and no connection yet, so the window shows that an agent is there before it
    /// asks anything. A window that is not there is not an error.
    pub async fn connect_only(&self) {
        let mut inner = self.inner.lock().await;
        if inner.connection.is_none() {
            inner.connection = self.connect().await.ok();
        }
    }
}

/// Sends one request and reads its reply.
async fn exchange(connection: &mut Connection, call: &Call) -> io::Result<Reply> {
    write_line(&mut connection.writer, call).await?;
    let line = read_line(&mut connection.reader, MAX_REPLY_BYTES)
        .await?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the window closed the connection",
            )
        })?;
    let reply: Reply = serde_json::from_str(&line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if reply.id != call.id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the reply is for another request",
        ));
    }
    Ok(reply)
}
