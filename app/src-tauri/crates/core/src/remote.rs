//! A picture on the web, fetched only because the person asked for it (a click on its chip, or the project's
//! "load web pictures" choice they turned on). The rules live here, pure over an [`Http`] seam: `https` only, at most
//! three redirects and each one `https` too, a picture type the bytes confirm, a size cap. The app crate supplies the
//! real HTTP client.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, IpcError};
use crate::state::StateData;

/// Largest picture fetched (bytes).
pub const MAX_PICTURE_BYTES: u64 = 20 * 1024 * 1024;
/// Redirects followed before giving up.
pub const MAX_REDIRECTS: usize = 3;
/// Longest address accepted.
pub const MAX_URL_CHARS: usize = 2048;
/// The per-project choice, in the profile only (`state::PROFILE_ONLY`).
pub const CHOICE_FILE: &str = "pictures.json";

/// What one request answered. `body` holds at most `limit + 1` bytes (so "more than the limit" can be told).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// The network, as the rules see it: one GET, no redirects followed, no cookies.
pub trait Http {
    /// `None`: no answer at all (offline, DNS, TLS, timeout).
    fn get(&self, url: &str, limit: u64) -> Option<Answer>;
}

/// A picture type PerMark shows from the web (SVG is not one: it is code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureType {
    Png,
    Jpeg,
    Gif,
    Webp,
    Avif,
    Bmp,
}

impl PictureType {
    fn of_mime(mime: &str) -> Option<Self> {
        let essence = mime
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        match essence.as_str() {
            "image/png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" | "image/pjpeg" => Some(Self::Jpeg),
            "image/gif" => Some(Self::Gif),
            "image/webp" => Some(Self::Webp),
            "image/avif" => Some(Self::Avif),
            "image/bmp" => Some(Self::Bmp),
            _ => None,
        }
    }

    /// Whether `bytes` start as a file of this type does.
    fn signed(self, bytes: &[u8]) -> bool {
        match self {
            Self::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpeg => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
            Self::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            Self::Webp => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
            Self::Avif => {
                bytes.len() >= 12
                    && &bytes[4..8] == b"ftyp"
                    && matches!(&bytes[8..12], b"avif" | b"avis")
            }
            Self::Bmp => bytes.starts_with(b"BM"),
        }
    }

    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Avif => "image/avif",
            Self::Bmp => "image/bmp",
        }
    }
}

/// A fetched picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub kind: PictureType,
    pub bytes: Vec<u8>,
}

fn refuse(kind: ErrorKind, detail: &str) -> IpcError {
    IpcError::new(kind).detail(detail)
}

/// `https://host/...` with a host and no user info; anything else is refused.
///
/// # Errors
/// `unsupported` (`https`: another scheme), `invalidData` (`address`: no host, user info, too long, spaces).
pub fn check_url(url: &str) -> Result<&str, IpcError> {
    let url = url.trim();
    if url.chars().count() > MAX_URL_CHARS
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(refuse(ErrorKind::InvalidData, "address"));
    }
    let Some(rest) = url
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
        .map(|_| &url[8..])
    else {
        return Err(refuse(ErrorKind::Unsupported, "https"));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') || authority.starts_with(':') {
        return Err(refuse(ErrorKind::InvalidData, "address"));
    }
    Ok(url)
}

/// Where a redirect leads, from the address it came from: an absolute `https` address, `//host/...`, or `/path`.
fn next_url(from: &str, location: &str) -> Result<String, IpcError> {
    let location = location.trim();
    let target = if location.starts_with("//") {
        format!("https:{location}")
    } else if location.starts_with('/') {
        let after = &from[8..];
        let host_end = after.find(['/', '?', '#']).unwrap_or(after.len());
        format!("{}{}", &from[..8 + host_end], location)
    } else {
        location.to_owned()
    };
    check_url(&target).map(str::to_owned)
}

/// Fetches the picture at `url` under the rules above.
///
/// # Errors
/// `unsupported` (not `https`, a redirect elsewhere, too many redirects), `invalidData` (not a picture PerMark shows,
/// bytes of another kind), `tooLarge` (over [`MAX_PICTURE_BYTES`]), `unavailable` (no answer, a status other than
/// 200).
pub fn fetch_picture(http: &dyn Http, url: &str) -> Result<Picture, IpcError> {
    let mut current = check_url(url)?.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        let answer = http
            .get(&current, MAX_PICTURE_BYTES)
            .ok_or_else(|| refuse(ErrorKind::Unavailable, "offline"))?;
        match answer.status {
            200 => return picture_of(answer),
            301 | 302 | 303 | 307 | 308 => {
                let location = answer
                    .location
                    .ok_or_else(|| refuse(ErrorKind::Unavailable, "status"))?;
                current = next_url(&current, &location)?;
            }
            _ => return Err(refuse(ErrorKind::Unavailable, "status")),
        }
    }
    Err(refuse(ErrorKind::Unsupported, "redirects"))
}

fn picture_of(answer: Answer) -> Result<Picture, IpcError> {
    if answer.body.len() as u64 > MAX_PICTURE_BYTES {
        return Err(refuse(ErrorKind::TooLarge, "picture"));
    }
    let kind = answer
        .content_type
        .as_deref()
        .and_then(PictureType::of_mime)
        .ok_or_else(|| refuse(ErrorKind::InvalidData, "picture"))?;
    if !kind.signed(&answer.body) {
        return Err(refuse(ErrorKind::InvalidData, "picture"));
    }
    Ok(Picture {
        kind,
        bytes: answer.body,
    })
}

/// The person's choice for one project: load web pictures by themselves (default off).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PicturesChoice {
    pub always: bool,
}

impl StateData for PicturesChoice {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests;
