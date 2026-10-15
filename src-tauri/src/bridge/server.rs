//! The window's side of the local socket: accepts connections and hands each request line to the [`Hub`].
//!
//! It runs on a thread of its own with a small Tokio runtime, so it needs no runtime from the application and stops
//! cleanly: [`BridgeServer::stop`] ends the accept loop, drops every connection and removes the socket.

use std::io;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use interprocess::local_socket::tokio::{Listener, Stream};
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use tokio::io::BufReader;
use tokio::sync::oneshot;

use super::hub::Hub;
use super::protocol::{read_line, write_line, Call, MAX_REQUEST_BYTES};
use super::socket::{Endpoint, ListenError};

/// More connections than this are turned away. An agent holds one; this leaves room for hooks and several agents.
const MAX_CONNECTIONS: usize = 16;
/// The pause after a failed accept, so a persistent failure does not spin.
const ACCEPT_RETRY: Duration = Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Listen(#[from] ListenError),
    #[error("cannot start the connection: {0}")]
    Thread(#[source] io::Error),
}

/// A running listener. Dropping it stops it.
pub struct BridgeServer {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl BridgeServer {
    /// Starts listening on `endpoint`. The socket exists when this returns, so a client that connects right after
    /// is not turned away.
    pub(crate) fn start(endpoint: &Endpoint, hub: Arc<Hub>) -> Result<Self, StartError> {
        // The runtime is made, used and dropped on the server's own thread, so starting works from any context
        // (a runtime cannot be dropped inside another one) and the thread reports how it went.
        let (ready, started) = mpsc::channel::<Result<oneshot::Sender<()>, StartError>>();
        let endpoint = endpoint.clone();
        let thread = thread::Builder::new()
            .name("trailmark-agent".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready.send(Err(StartError::Thread(error)));
                        return;
                    }
                };
                let listener = {
                    let _entered = runtime.enter();
                    endpoint.listen()
                };
                let listener = match listener {
                    Ok(listener) => listener,
                    Err(error) => {
                        let _ = ready.send(Err(error.into()));
                        return;
                    }
                };
                let (stop, stopped) = oneshot::channel();
                if ready.send(Ok(stop)).is_err() {
                    return;
                }
                runtime.block_on(accept_loop(listener, hub, stopped));
                // What is still being served is dropped with the runtime, closing its connection.
            })
            .map_err(StartError::Thread)?;
        match started.recv() {
            Ok(Ok(stop)) => Ok(Self {
                stop: Some(stop),
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(StartError::Thread(io::Error::other(
                    "the connection's thread ended while starting",
                )))
            }
        }
    }

    /// Stops listening, closes every connection and waits for the thread to finish. Safe to call more than once.
    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn accept_loop(listener: Listener, hub: Arc<Hub>, mut stopped: oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            accepted = listener.accept() => match accepted {
                Ok(stream) => {
                    if hub.connections() >= MAX_CONNECTIONS {
                        continue; // dropped: the client sees the connection close
                    }
                    tokio::spawn(serve_connection(stream, Arc::clone(&hub)));
                }
                Err(_) => tokio::time::sleep(ACCEPT_RETRY).await,
            },
        }
    }
}

/// Keeps the hub's count of connections true for as long as the connection is served, however it ends.
struct Counted(Arc<Hub>);

impl Counted {
    fn new(hub: Arc<Hub>) -> Self {
        hub.connection_opened();
        Self(hub)
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.connection_closed();
    }
}

/// Answers the requests of one connection, one line each, until the client closes it or sends nonsense that is
/// too large to be a request.
async fn serve_connection(stream: Stream, hub: Arc<Hub>) {
    let _counted = Counted::new(Arc::clone(&hub));
    let (recv, mut send) = stream.split();
    let mut lines = BufReader::new(recv);
    while let Ok(Some(line)) = read_line(&mut lines, MAX_REQUEST_BYTES).await {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Call>(&line) {
            Ok(call) => hub.handle(call),
            Err(_) => Hub::reject(&line),
        };
        if write_line(&mut send, &reply).await.is_err() {
            break;
        }
    }
}
