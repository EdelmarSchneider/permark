//! Turning file bytes into text: BOMs, UTF-16, lossy UTF-8, binary sniffing and line endings.

/// How many leading bytes are inspected for NUL when deciding that a file is binary.
const SNIFF_LEN: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decoded {
    Text {
        /// Text with `\n` line endings.
        text: String,
        /// What the reader should know when the file was not plain UTF-8: a code the UI words itself
        /// (`invalidUtf8`, `utf16Le`, `utf16LeLossy`, `utf16Be`, `utf16BeLossy`).
        note: Option<String>,
    },
    Binary,
}

/// Decodes a file. UTF-8 BOMs are dropped silently; UTF-16 (with BOM) and invalid UTF-8 produce a note.
pub(crate) fn decode(bytes: &[u8]) -> Decoded {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return from_utf8(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return from_utf16(rest, encoding_rs::UTF_16LE, "utf16Le");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return from_utf16(rest, encoding_rs::UTF_16BE, "utf16Be");
    }
    if bytes[..bytes.len().min(SNIFF_LEN)].contains(&0) {
        return Decoded::Binary;
    }
    from_utf8(bytes)
}

fn from_utf8(bytes: &[u8]) -> Decoded {
    match std::str::from_utf8(bytes) {
        Ok(s) => Decoded::Text {
            text: normalize_newlines(s),
            note: None,
        },
        Err(_) => Decoded::Text {
            text: normalize_newlines(&String::from_utf8_lossy(bytes)),
            note: Some("invalidUtf8".to_owned()),
        },
    }
}

/// `code` is the note for a clean conversion; a conversion that lost characters adds `Lossy`.
fn from_utf16(bytes: &[u8], enc: &'static encoding_rs::Encoding, code: &str) -> Decoded {
    let (text, had_errors) = enc.decode_without_bom_handling(bytes);
    let note = if had_errors {
        format!("{code}Lossy")
    } else {
        code.to_owned()
    };
    Decoded::Text {
        text: normalize_newlines(&text),
        note: Some(note),
    }
}

/// `\r\n` and lone `\r` become `\n`.
pub(crate) fn normalize_newlines(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_owned();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(d: &Decoded) -> (&str, Option<&str>) {
        match d {
            Decoded::Text { text, note } => (text.as_str(), note.as_deref()),
            Decoded::Binary => panic!("expected text"),
        }
    }

    #[test]
    fn plain_utf8_has_no_note() {
        let d = decode("héllo\n".as_bytes());
        assert_eq!(text(&d), ("héllo\n", None));
    }

    #[test]
    fn utf8_bom_is_stripped_silently() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(b"# Title");
        assert_eq!(text(&decode(&b)), ("# Title", None));
    }

    #[test]
    fn utf16_le_and_be_with_bom_are_converted() {
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("# Olá\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("# Olá\r\n".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        let le = decode(&le);
        let (t, n) = text(&le);
        assert_eq!(t, "# Olá\n");
        assert_eq!(n, Some("utf16Le"));
        let be = decode(&be);
        let (t, n) = text(&be);
        assert_eq!(t, "# Olá\n");
        assert_eq!(n, Some("utf16Be"));
    }

    #[test]
    fn invalid_utf8_is_lossy_with_a_note() {
        let (t, n) = {
            let d = decode(b"caf\xE9 ok");
            let Decoded::Text { text, note } = d else {
                panic!("expected text")
            };
            (text, note)
        };
        assert_eq!(t, "caf\u{FFFD} ok");
        assert_eq!(n.as_deref(), Some("invalidUtf8"));
    }

    #[test]
    fn nul_in_the_first_8kb_means_binary() {
        assert_eq!(decode(b"PK\x03\x04\0\0abc"), Decoded::Binary);
        let mut late = vec![b'a'; SNIFF_LEN + 10];
        late[SNIFF_LEN + 5] = 0;
        assert!(matches!(decode(&late), Decoded::Text { .. }));
    }

    #[test]
    fn crlf_and_cr_are_normalized() {
        assert_eq!(text(&decode(b"a\r\nb\rc\n")), ("a\nb\nc\n", None));
    }

    #[test]
    fn empty_file_is_empty_text() {
        assert_eq!(text(&decode(b"")), ("", None));
    }
}
