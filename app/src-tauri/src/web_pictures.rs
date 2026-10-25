//! Pictures on the web, fetched only when the person asks (`permark_core::remote` holds the rules): the HTTP client
//! (`ureq` over rustls with the operating system's certificates, no redirects followed by the client, no cookies, only
//! public addresses connected to) and the commands of the reading view.

use std::fmt;
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use permark_core::error::IpcError;
use permark_core::remote::{self, Answer, Http, NoAnswer, PicturesChoice};
use permark_core::state::Loaded;
use permark_core::workspace::ProjectCore;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// Marks the refusal of [`PublicOnly`] inside a `ureq` error.
#[derive(Debug)]
struct LocalAddress;

impl fmt::Display for LocalAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the name resolves to an address that is not public")
    }
}

impl std::error::Error for LocalAddress {}

/// ureq's own resolver, refusing a host with any address that is not public (`remote::is_public`). ureq connects only
/// to the addresses returned here, so the address checked is the address connected to (no DNS rebinding window); each
/// redirect hop is its own request and is resolved again. The system proxy's own address is the person's setup and is
/// not checked; through a proxy the proxy resolves the picture's host, so only `remote::check_url` applies there.
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

struct Ureq {
    agent: OnceLock<ureq::Agent>,
}

impl Ureq {
    fn agent(&self) -> &ureq::Agent {
        self.agent
            .get_or_init(|| agent(ureq::Proxy::try_from_env()))
    }
}

/// The client: rustls with the operating system's certificates, no redirects, no cookies, timeouts, [`PublicOnly`];
/// `proxy` = the system proxy in the app (`HTTPS_PROXY` | `ALL_PROXY`, `NO_PROXY`).
fn agent(proxy: Option<ureq::Proxy>) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(proxy)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::default(), PublicOnly::default())
}

impl Http for Ureq {
    fn get(&self, url: &str, limit: u64) -> Result<Answer, NoAnswer> {
        let mut response = self
            .agent()
            .get(url)
            .header(
                "Accept",
                "image/avif,image/webp,image/png,image/jpeg,image/gif,image/bmp",
            )
            .call()
            .map_err(|error| match error {
                ureq::Error::Other(inner) if inner.is::<LocalAddress>() => NoAnswer::LocalAddress,
                _ => NoAnswer::Offline,
            })?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let location = header("location");
        let content_type = header("content-type");
        let status = response.status().as_u16();
        let mut body = Vec::new();
        if status == 200 {
            // one byte more than the cap tells "too large" without reading the rest
            response
                .body_mut()
                .as_reader()
                .take(limit + 1)
                .read_to_end(&mut body)
                .map_err(|_| NoAnswer::Offline)?;
        }
        Ok(Answer {
            status,
            location,
            content_type,
            body,
        })
    }
}

static HTTP: Ureq = Ureq {
    agent: OnceLock::new(),
};

/// The bytes of the picture at `url`, fetched now under the rules of [`remote::fetch_picture`].
///
/// # Errors
/// See [`remote::fetch_picture`].
pub fn fetch(url: &str) -> Result<Vec<u8>, IpcError> {
    Ok(remote::fetch_picture(&HTTP, url)?.bytes)
}

/// Whether web pictures load by themselves in the project of `core` (the person's choice, profile only).
#[must_use]
pub fn always(core: &ProjectCore) -> bool {
    match core
        .store()
        .profile_file::<PicturesChoice>(remote::CHOICE_FILE)
        .load()
    {
        Loaded::Found(choice) => choice.always,
        Loaded::Missing | Loaded::Recovered { .. } => false,
    }
}

/// Sets that choice.
///
/// # Errors
/// `io` when the profile cannot be written.
pub fn set_always(core: &ProjectCore, always: bool) -> Result<(), IpcError> {
    Ok(core
        .store()
        .profile_file::<PicturesChoice>(remote::CHOICE_FILE)
        .save(&PicturesChoice { always })?)
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use permark_core::remote::{Http, NoAnswer};

    use super::{agent, Ureq};

    fn client(proxy: Option<ureq::Proxy>) -> Ureq {
        Ureq {
            agent: OnceLock::from(agent(proxy)),
        }
    }

    /// A listener on this computer that tells whether anything connected to it.
    fn listener() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    fn nobody_came(listener: &TcpListener) -> bool {
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
    }

    #[test]
    fn a_local_address_is_never_connected_to() {
        let (listener, port) = listener();
        let http = client(None);
        for url in [
            format!("https://127.0.0.1:{port}/a.png"),
            format!("https://localhost:{port}/a.png"),
            format!("https://[::ffff:127.0.0.1]:{port}/a.png"),
            format!("https://[::1]:{port}/a.png"),
        ] {
            assert_eq!(
                http.get(&url, 10).unwrap_err(),
                NoAnswer::LocalAddress,
                "{url}"
            );
        }
        assert!(nobody_came(&listener), "no connection was made");
    }

    #[test]
    fn a_host_on_no_proxy_is_resolved_and_checked_here() {
        let (proxy_listener, proxy_port) = listener();
        let (listener, port) = listener();
        let proxy = ureq::Proxy::builder(ureq::ProxyProtocol::Http)
            .host("127.0.0.1")
            .port(proxy_port)
            .no_proxy("127.0.0.1")
            .build()
            .unwrap();
        let http = client(Some(proxy));
        assert_eq!(
            http.get(&format!("https://127.0.0.1:{port}/a.png"), 10)
                .unwrap_err(),
            NoAnswer::LocalAddress
        );
        assert!(nobody_came(&proxy_listener) && nobody_came(&listener));
    }

    #[test]
    fn the_proxy_itself_may_be_on_this_computer() {
        let (listener, port) = listener();
        let proxy = ureq::Proxy::new(&format!("http://127.0.0.1:{port}")).unwrap();
        let http = client(Some(proxy));
        let asking = std::thread::spawn(move || http.get("https://picture.example/a.png", 10));
        let deadline = Instant::now() + Duration::from_secs(20);
        let came = loop {
            match listener.accept() {
                // the proxy answers nothing: the request ends without an answer
                Ok((stream, _)) => {
                    drop(stream);
                    break true;
                }
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break false,
            }
        };
        assert!(came, "the request went to the proxy");
        assert_eq!(asking.join().unwrap().unwrap_err(), NoAnswer::Offline);
    }

    /// Real network, run by hand: `cargo test -p permark web_pictures -- --ignored`.
    #[test]
    #[ignore = "asks the real network"]
    fn a_real_https_picture_comes_and_a_page_does_not() {
        let png = super::fetch(
            "https://raw.githubusercontent.com/EdelmarSchneider/permark/main/docs/assets/screenshot-light.png",
        )
        .unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        let page = super::fetch("https://github.com/").unwrap_err();
        // a web page is never shown as a picture (refused by type, or by its status for this Accept)
        assert!(matches!(
            page.kind,
            permark_core::error::ErrorKind::InvalidData
                | permark_core::error::ErrorKind::Unavailable
        ));
    }
}
