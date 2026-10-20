//! Links of Markdown documents: where each destination sits in the text ([`parse`]), where a link leads
//! ([`resolve`]).

mod parse;
mod resolve;
#[cfg(test)]
mod tests;

pub use parse::{parse, Dest, Form, Kind};
pub use resolve::{join_relative, local_target, resolve, Target};

/// Decodes `%XX` escapes (invalid sequences are kept as written; invalid UTF-8 becomes U+FFFD).
#[must_use]
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).and_then(|b| char::from(*b).to_digit(16));
            let lo = bytes.get(i + 2).and_then(|b| char::from(*b).to_digit(16));
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'%'));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The URL scheme of `href` (`https`, `mailto`, ...), lowercased, if it has one.
#[must_use]
pub fn scheme_of(href: &str) -> Option<String> {
    let colon = href.find(':')?;
    let scheme = &href[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then(|| scheme.to_ascii_lowercase())
}
