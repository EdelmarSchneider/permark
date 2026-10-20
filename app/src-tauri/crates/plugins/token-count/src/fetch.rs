//! The one network call of the plugin: a GET of a public file, streamed. Nothing calls it before the person confirmed
//! the download.

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

/// Why a fetch gave nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// No answer: no connection, DNS, TLS, a stalled or dropped transfer.
    Offline,
    /// The server answered with this status instead of the file.
    Status(u16),
}

/// A source of bytes by address. Blocking; the plugin calls it on its own thread.
pub trait Fetch: Send + Sync {
    /// Streams the body of `url` to `sink` in chunks until it ends or `sink` returns `false`.
    ///
    /// # Errors
    /// `Offline` | `Status`.
    fn get(&self, url: &str, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), FetchError>;
}

/// The real fetch: `ureq` over rustls with the operating system's certificate store. No redirects are followed (the
/// official addresses answer directly), no cookies, no body decompression.
pub struct UreqFetch {
    agent: OnceLock<ureq::Agent>,
}

impl UreqFetch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            agent: OnceLock::new(),
        }
    }

    /// Built on first use (turning the plugin on costs nothing).
    fn agent(&self) -> &ureq::Agent {
        self.agent.get_or_init(Self::build)
    }

    fn build() -> ureq::Agent {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            // a stalled transfer ends (and a stop request is heard) within this time
            .timeout_recv_body(Some(Duration::from_secs(8)))
            .build();
        ureq::Agent::new_with_config(config)
    }
}

impl Default for UreqFetch {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetch for UreqFetch {
    fn get(&self, url: &str, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), FetchError> {
        let mut response = self
            .agent()
            .get(url)
            .call()
            .map_err(|_| FetchError::Offline)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(FetchError::Status(status));
        }
        let mut reader = response.body_mut().as_reader();
        let mut buffer = vec![0_u8; 32 * 1024];
        loop {
            let read = reader.read(&mut buffer).map_err(|_| FetchError::Offline)?;
            if read == 0 {
                return Ok(());
            }
            if !sink(&buffer[..read]) {
                return Ok(());
            }
        }
    }
}
