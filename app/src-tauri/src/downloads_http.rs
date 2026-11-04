//! The app's network client + credential store for core seams: one `ureq` agent builder (rustls with the operating
//! system's certificates, no redirects followed by the client, no cookies, only public addresses connected to) shared by
//! web pictures and verified downloads; the downloads [`Transport`] over it; the OS keychain [`SecretStore`].

use std::fmt;
use std::io::Read;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use permark_core::downloads::{
    DownloadError, DownloadKit, Request, Response, Secret, SecretError, SecretStore, Source,
    Transport,
};
use permark_core::plugins::ServiceLease;
use permark_core::remote;
use permark_core::workspace::CoreEnv;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport as Connection,
};

/// Marks the refusal of [`PublicOnly`] inside a `ureq` error.
#[derive(Debug)]
struct LocalAddress;

impl fmt::Display for LocalAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the name resolves to an address that is not public")
    }
}

impl std::error::Error for LocalAddress {}

/// `error` = [`PublicOnly`] refused the host (nothing connected).
pub(crate) fn refused_local(error: &ureq::Error) -> bool {
    matches!(error, ureq::Error::Other(inner) if inner.is::<LocalAddress>())
}

/// ureq's own resolver, refusing a host with any address that is not public (`remote::is_public`). ureq connects only
/// to the addresses returned here, so the address checked is the address connected to (no DNS rebinding window); each
/// new connection is resolved and checked, and a reused connection was checked when it opened (redirect hops
/// included). The system proxy's own address is the person's setup and is not checked; through a proxy the proxy
/// resolves the target host, so only `remote::check_url` applies there.
///
/// Uses ureq's `unversioned` API (`Resolver`, `DefaultResolver`, `Connector`, `Agent::with_parts`), which does not
/// follow semver: a ureq update may need this adjusted (the tests here and in `web_pictures` fail if a check stops
/// working).
#[derive(Debug, Default)]
struct PublicOnly(DefaultResolver);

impl Resolver for PublicOnly {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addrs = self.0.resolve(uri, config, timeout)?;
        let proxy = config.proxy().is_some_and(|proxy| proxy.uri() == uri);
        if proxy || addrs.iter().all(|addr| remote::is_public(addr.ip())) {
            Ok(addrs)
        } else {
            Err(ureq::Error::Other(Box::new(LocalAddress)))
        }
    }
}

/// How long a request may take (connect: 15 s in both).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Patience {
    /// Whole request, connect to last body byte (small answers: web pictures).
    Total(Duration),
    /// No total (large files, D-11): every wait for bytes in | out ends after `idle` without progress.
    Idle(Duration),
}

/// The client: rustls with the operating system's certificates, no redirects, no cookies, [`PublicOnly`], `patience`;
/// `proxy` = the system proxy in the app (`HTTPS_PROXY` | `ALL_PROXY`, `NO_PROXY`).
pub(crate) fn agent(proxy: Option<ureq::Proxy>, patience: Patience) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(proxy)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)));
    // ureq `unversioned` API (see `PublicOnly`): may need adjusting on ureq updates
    match patience {
        Patience::Total(total) => ureq::Agent::with_parts(
            config.timeout_global(Some(total)).build(),
            DefaultConnector::default(),
            PublicOnly::default(),
        ),
        Patience::Idle(idle) => ureq::Agent::with_parts(
            config.build(),
            DefaultConnector::default().chain(IdleCap(idle)),
            PublicOnly::default(),
        ),
    }
}

/// Last link of the connector chain: caps every single wait of the connection at the idle time (ureq's own
/// `timeout_recv_body` bounds the whole body, wrong for multi-GB files).
#[derive(Debug)]
struct IdleCap(Duration);

impl Connector<Box<dyn Connection>> for IdleCap {
    type Out = Idle;

    fn connect(
        &self,
        _details: &ConnectionDetails,
        chained: Option<Box<dyn Connection>>,
    ) -> Result<Option<Idle>, ureq::Error> {
        Ok(chained.map(|inner| Idle {
            inner,
            idle: self.0,
        }))
    }
}

/// A connection whose waits end after `idle` without bytes.
#[derive(Debug)]
struct Idle {
    inner: Box<dyn Connection>,
    idle: Duration,
}

impl Idle {
    fn capped(&self, timeout: NextTimeout) -> NextTimeout {
        NextTimeout {
            after: timeout.after.min(self.idle.into()),
            reason: timeout.reason,
        }
    }
}

impl Connection for Idle {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        let timeout = self.capped(timeout);
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let timeout = self.capped(timeout);
        self.inner.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

/// Download waits: a stalled transfer ends (and a cancel is heard) within this time.
const DOWNLOAD_IDLE: Duration = Duration::from_secs(10);

/// The downloads [`Transport`]: one GET, body streamed (never buffered). Agent built on first request.
#[derive(Debug, Default)]
struct UreqTransport {
    agent: OnceLock<ureq::Agent>,
}

impl UreqTransport {
    fn agent(&self) -> &ureq::Agent {
        self.agent
            .get_or_init(|| agent(ureq::Proxy::try_from_env(), Patience::Idle(DOWNLOAD_IDLE)))
    }
}

impl Transport for UreqTransport {
    fn get(&self, request: &Request<'_>) -> Result<Response, DownloadError> {
        let mut call = self.agent().get(request.url);
        if let Some(from) = request.from {
            call = call.header("Range", format!("bytes={from}-"));
        }
        if let Some(secret) = request.auth {
            call = call.header("Authorization", format!("Bearer {}", secret.expose()));
        }
        // the error text is never shown: it may echo request details
        let response = call.call().map_err(|error| {
            if refused_local(&error) {
                DownloadError::HostNotAllowed
            } else {
                DownloadError::Offline
            }
        })?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let location = header("location");
        let content_range = header("content-range");
        let content_length = header("content-length").and_then(|v| v.trim().parse().ok());
        let status = response.status().as_u16();
        let body: Box<dyn Read + Send> = Box::new(response.into_body().into_reader());
        Ok(Response {
            status,
            location,
            content_range,
            content_length,
            body,
        })
    }
}

/// OS credential store slot service (Windows Credential Manager, macOS Keychain, Secret Service); user = source key.
const KEYCHAIN_SERVICE: &str = "PerMark";

/// Credentials in the OS keychain (`keyring`). Store opened on first use; errors carry no value.
#[derive(Debug, Default)]
struct Keychain;

impl Keychain {
    fn entry(source: &Source) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(KEYCHAIN_SERVICE, source.key()).map_err(|error| unavailable(&error))
    }
}

/// Keychain failure -> [`SecretError`]; text = category only (never a value | encoded bytes).
fn unavailable(error: &keyring::Error) -> SecretError {
    let reason = match error {
        keyring::Error::NoStorageAccess(_) | keyring::Error::NoDefaultStore => "no access",
        keyring::Error::PlatformFailure(_) => "platform failure",
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => "bad stored value",
        _ => "refused",
    };
    SecretError::Unavailable(reason.to_owned())
}

impl SecretStore for Keychain {
    fn get(&self, source: &Source) -> Result<Option<Secret>, SecretError> {
        match Self::entry(source)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(unavailable(&error)),
        }
    }

    fn set(&self, source: &Source, secret: Secret) -> Result<(), SecretError> {
        Self::entry(source)?
            .set_password(secret.expose())
            .map_err(|error| unavailable(&error))
    }

    fn clear(&self, source: &Source) -> Result<(), SecretError> {
        match Self::entry(source)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(unavailable(&error)),
        }
    }
}

/// Offers the downloads network + credential store for the app's whole run (nothing opened before first use).
pub fn provide(env: &CoreEnv) -> ServiceLease {
    env.services().provide(Arc::new(DownloadKit {
        transport: Arc::new(UreqTransport::default()),
        secrets: Arc::new(Keychain),
    }))
}

#[cfg(test)]
mod tests;
