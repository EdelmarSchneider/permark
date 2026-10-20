//! Pictures on the web, fetched only when the person asks (`permark_core::remote` holds the rules): the HTTP client
//! (`ureq` over rustls with the operating system's certificates, no redirects followed by the client, no cookies) and
//! the commands of the reading view.

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use permark_core::error::IpcError;
use permark_core::remote::{self, Answer, Http, PicturesChoice};
use permark_core::state::Loaded;
use permark_core::workspace::ProjectCore;

struct Ureq {
    agent: OnceLock<ureq::Agent>,
}

impl Ureq {
    fn agent(&self) -> &ureq::Agent {
        self.agent.get_or_init(|| {
            let tls = ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build();
            let config = ureq::Agent::config_builder()
                .tls_config(tls)
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_global(Some(Duration::from_secs(30)))
                .build();
            ureq::Agent::new_with_config(config)
        })
    }
}

impl Http for Ureq {
    fn get(&self, url: &str, limit: u64) -> Option<Answer> {
        let mut response = self
            .agent()
            .get(url)
            .header(
                "Accept",
                "image/avif,image/webp,image/png,image/jpeg,image/gif,image/bmp",
            )
            .call()
            .ok()?;
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
                .ok()?;
        }
        Some(Answer {
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
