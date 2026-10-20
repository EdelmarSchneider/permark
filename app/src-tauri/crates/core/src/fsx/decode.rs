//! Bytes -> text + the facts a surgical save needs (encoding, BOM, line endings, final newline). Pure.

use serde::Serialize;
use ts_rs::TS;

/// Head bytes checked for NUL (binary sniff).
pub const BINARY_SNIFF: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LineEnding {
    /// No line break at all.
    None,
    Lf,
    CrLf,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// Text without BOM.
    pub text: String,
    pub encoding: Encoding,
    /// Invalid sequences were replaced with U+FFFD (saving would change bytes).
    pub lossy: bool,
    pub line_ending: LineEnding,
    pub trailing_newline: bool,
}

/// Decodes file bytes; `None` = binary (NUL in the head without a UTF-16 BOM).
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    let (encoding, body) = if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        (Encoding::Utf8Bom, rest)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        (Encoding::Utf16Le, rest)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        (Encoding::Utf16Be, rest)
    } else {
        (Encoding::Utf8, bytes)
    };
    let (text, lossy) = match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            if body[..body.len().min(BINARY_SNIFF)].contains(&0) {
                return None;
            }
            match String::from_utf8_lossy(body) {
                std::borrow::Cow::Borrowed(text) => (text.to_owned(), false),
                std::borrow::Cow::Owned(text) => (text, true),
            }
        }
        Encoding::Utf16Le => utf16(body, u16::from_le_bytes),
        Encoding::Utf16Be => utf16(body, u16::from_be_bytes),
    };
    Some(Decoded {
        line_ending: line_ending(&text),
        trailing_newline: text.ends_with('\n'),
        text,
        encoding,
        lossy,
    })
}

fn utf16(body: &[u8], unit: fn([u8; 2]) -> u16) -> (String, bool) {
    let odd = body.len() % 2 == 1;
    let units = body.as_chunks::<2>().0.iter().map(|pair| unit(*pair));
    let mut lossy = odd;
    let mut text: String = char::decode_utf16(units)
        .map(|c| {
            c.unwrap_or_else(|_| {
                lossy = true;
                char::REPLACEMENT_CHARACTER
            })
        })
        .collect();
    if odd {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    (text, lossy)
}

#[must_use]
pub fn line_ending(text: &str) -> LineEnding {
    let bytes = text.as_bytes();
    let (mut lf, mut crlf) = (false, false);
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            if i > 0 && bytes[i - 1] == b'\r' {
                crlf = true;
            } else {
                lf = true;
            }
            if lf && crlf {
                return LineEnding::Mixed;
            }
        }
    }
    match (lf, crlf) {
        (false, false) => LineEnding::None,
        (true, false) => LineEnding::Lf,
        (false, true) => LineEnding::CrLf,
        (true, true) => LineEnding::Mixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_and_bom() {
        let d = decode(b"# T\nx\n").unwrap();
        assert_eq!(
            (d.encoding, d.lossy, d.trailing_newline),
            (Encoding::Utf8, false, true)
        );
        assert_eq!(d.line_ending, LineEnding::Lf);
        let d = decode(b"\xEF\xBB\xBFa\r\nb").unwrap();
        assert_eq!(d.text, "a\r\nb");
        assert_eq!(
            (d.encoding, d.line_ending, d.trailing_newline),
            (Encoding::Utf8Bom, LineEnding::CrLf, false)
        );
    }

    #[test]
    fn mixed_and_none_line_endings() {
        assert_eq!(line_ending("a\r\nb\nc"), LineEnding::Mixed);
        assert_eq!(line_ending("a\nb\r\n"), LineEnding::Mixed);
        assert_eq!(line_ending("single"), LineEnding::None);
        assert_eq!(line_ending("lone\rcr"), LineEnding::None);
        assert_eq!(line_ending(""), LineEnding::None);
    }

    #[test]
    fn utf16_both_orders() {
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("hé\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let d = decode(&le).unwrap();
        assert_eq!(
            (d.text.as_str(), d.encoding, d.line_ending),
            ("hé\r\n", Encoding::Utf16Le, LineEnding::CrLf)
        );
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("文".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode(&be).unwrap().text, "文");
    }

    #[test]
    fn malformed_input_is_lossy_never_a_panic() {
        let d = decode(b"ok \xC3\x28 bad").unwrap();
        assert!(d.lossy);
        assert!(d.text.contains('\u{FFFD}'));
        // odd byte count + lone surrogate in UTF-16
        let d = decode(&[0xFF, 0xFE, 0x00, 0xD8, 0x41]).unwrap();
        assert!(d.lossy);
        // truncated BOM-like prefixes are plain bytes
        assert!(decode(b"\xEF\xBB").unwrap().lossy);
    }

    #[test]
    fn nul_in_head_is_binary_but_not_later() {
        assert!(decode(b"abc\0def").is_none());
        let mut late = vec![b'a'; BINARY_SNIFF];
        late.push(0);
        assert!(decode(&late).is_some());
        assert!(decode(b"").is_some());
    }
}
