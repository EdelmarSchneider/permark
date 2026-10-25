//! A picture on the web, fetched only because the person asked for it (a click on its chip, or the project's
//! "load web pictures" choice they turned on). The rules live here, pure over an [`Http`] seam: `https` only, at most
//! three redirects and each one `https` too, public addresses only (never this computer or the local network), a
//! picture type the bytes confirm, a size cap. The app crate supplies the real HTTP client, whose resolver refuses a
//! name that resolves to an address that is not [`is_public`].

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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

/// Why one request got no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoAnswer {
    /// Offline, DNS, TLS, timeout.
    Offline,
    /// The host's name resolves to an address that is not [`is_public`]; nothing was connected to.
    LocalAddress,
}

/// The network, as the rules see it: one GET, no redirects followed, no cookies, only public addresses connected to.
pub trait Http {
    /// # Errors
    /// [`NoAnswer`]: no answer at all, or a name that resolves to an address that is not public.
    fn get(&self, url: &str, limit: u64) -> Result<Answer, NoAnswer>;
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

/// A host that is this computer or the local network: `denied`, the host as written.
fn local(host: &str) -> IpcError {
    refuse(ErrorKind::Denied, "address").subject(host)
}

/// Whether `ip` is an address on the open internet: not this computer, the local network, link-local (cloud metadata
/// `169.254.169.254` included), shared (CGNAT), documentation, benchmarking, multicast or reserved. An IPv6 form that
/// carries an IPv4 address (mapped, compatible, translated, NAT64, 6to4) is judged by that address.
#[must_use]
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => match carried_v4(v6) {
            Some(v4) => public_v4(v4),
            None => public_v6(v6),
        },
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let not_public = a == 0 // "this network"
        || a == 10
        || a == 127
        || a >= 224 // multicast, reserved, broadcast
        || (a == 100 && (64..128).contains(&b)) // shared (CGNAT)
        || (a == 169 && b == 254) // link-local, cloud metadata
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && (c == 0 || c == 2)) // protocol assignments, documentation
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || (a == 198 && b == 51 && c == 100) // documentation
        || (a == 203 && b == 0 && c == 113); // documentation
    !not_public
}

/// The IPv4 address an IPv6 address carries: `::a.b.c.d` (compatible; `::` and `::1` give `0.0.0.x`, not public),
/// `::ffff:a.b.c.d` (mapped), `::ffff:0:a.b.c.d` (translated), `64:ff9b::a.b.c.d` (NAT64), `2002:aabb:ccdd::/48`
/// (6to4).
fn carried_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let o = ip.octets();
    let at = |i: usize| Ipv4Addr::new(o[i], o[i + 1], o[i + 2], o[i + 3]);
    let compatible_or_mapped = s[..5] == [0; 5] && (s[5] == 0 || s[5] == 0xffff);
    let translated = s[..4] == [0; 4] && s[4] == 0xffff && s[5] == 0;
    let nat64 = s[..6] == [0x64, 0xff9b, 0, 0, 0, 0];
    if compatible_or_mapped || translated || nat64 {
        Some(at(12))
    } else if s[0] == 0x2002 {
        Some(at(2))
    } else {
        None
    }
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let not_public = ip.is_unspecified()
        || ip.is_loopback()
        || (s[0] & 0xfe00) == 0xfc00 // unique local (ULA)
        || (s[0] & 0xffc0) == 0xfe80 // link-local
        || (s[0] & 0xffc0) == 0xfec0 // site-local (deprecated)
        || (s[0] & 0xff00) == 0xff00 // multicast
        || s[..4] == [0x100, 0, 0, 0] // discard only
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1); // local-use NAT64
    !not_public
}

/// The host of an authority as written: `[v6]` with its brackets, else everything before a port.
fn host_part(authority: &str) -> &str {
    if authority.starts_with('[') {
        authority
            .find(']')
            .map_or(authority, |end| &authority[..=end])
    } else {
        authority.split(':').next().unwrap_or_default()
    }
}

/// The host of an `https` address may be asked: not `localhost` | `*.localhost`, a number only as a plain dotted
/// IPv4 address (`127.1`, `2130706433`, `0x7f.1` and leading zeros are read as addresses by resolvers), an IP literal
/// only when [`is_public`]. A name is checked again once resolved, by the app's resolver.
fn check_host(authority: &str) -> Result<(), IpcError> {
    let host = host_part(authority);
    let ip = if let Some(inside) = host.strip_prefix('[') {
        // a zone id (`%25eth0`) or anything else that is not an IPv6 address is refused here
        let ip = inside
            .strip_suffix(']')
            .and_then(|v6| v6.parse::<Ipv6Addr>().ok())
            .ok_or_else(|| refuse(ErrorKind::InvalidData, "address"))?;
        IpAddr::V6(ip)
    } else {
        let name = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
        if name == "localhost" || name.ends_with(".localhost") {
            return Err(local(host));
        }
        // no top-level domain starts with a digit: such a host is an address in some notation
        let last = name.rsplit('.').next().unwrap_or_default();
        if !last.starts_with(|c: char| c.is_ascii_digit()) {
            return Ok(());
        }
        let ip = name
            .parse::<Ipv4Addr>()
            .map_err(|_| refuse(ErrorKind::InvalidData, "address"))?;
        IpAddr::V4(ip)
    };
    if is_public(ip) {
        Ok(())
    } else {
        Err(local(host))
    }
}

/// `https://host/...` with a host and no user info, the host not this computer or the local network; anything else is
/// refused.
///
/// # Errors
/// `unsupported` (`https`: another scheme), `invalidData` (`address`: no host, user info, too long, spaces, a numeric
/// host that is not a plain IPv4 address), `denied` (`address`, subject the host: `localhost`, an address that is not
/// [`is_public`]).
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
    check_host(authority)?;
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
/// `unsupported` (not `https`, a redirect elsewhere, too many redirects), `denied` (`address`: a host, or a name that
/// resolves to an address, that is not public; on any hop), `invalidData` (not a picture PerMark shows, bytes of
/// another kind), `tooLarge` (over [`MAX_PICTURE_BYTES`]), `unavailable` (no answer, a status other than 200).
pub fn fetch_picture(http: &dyn Http, url: &str) -> Result<Picture, IpcError> {
    let mut current = check_url(url)?.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        let answer = http
            .get(&current, MAX_PICTURE_BYTES)
            .map_err(|no| match no {
                NoAnswer::Offline => refuse(ErrorKind::Unavailable, "offline"),
                NoAnswer::LocalAddress => {
                    let authority = current[8..].split(['/', '?', '#']).next();
                    local(host_part(authority.unwrap_or_default()))
                }
            })?;
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
