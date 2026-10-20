//! The formats viewers read: plain text (and its JSON | XML kin: same body, another viewer) and media.
//! Plugin crates (`text-view`, `json-view`, `xml-view`, `media`) hand these over as contributions.

use super::{ext_of, name_of, Body, Claim, Format, FormatId, MediaKind, RenderCx, Rendered};

/// Text shown by a viewer chosen by the format id; the UI picks the viewer, the body is the text.
#[derive(Debug, Clone, Copy)]
pub struct TextViewer {
    id: FormatId,
    extensions: &'static [&'static str],
    /// Claims every file weakly (the viewer of last resort).
    fallback: bool,
    /// Language for highlighting when the extension does not say it.
    language: Option<&'static str>,
}

impl TextViewer {
    /// Any text.
    #[must_use]
    pub const fn plain() -> Self {
        Self {
            id: FormatId::TEXT,
            extensions: &[],
            fallback: true,
            language: None,
        }
    }

    #[must_use]
    pub const fn json() -> Self {
        Self {
            id: FormatId::JSON,
            extensions: &["json", "jsonc", "geojson", "webmanifest"],
            fallback: false,
            language: Some("json"),
        }
    }

    #[must_use]
    pub const fn xml() -> Self {
        Self {
            id: FormatId::XML,
            extensions: &[
                "xml", "xsd", "xsl", "xslt", "svg", "plist", "csproj", "rss", "atom",
            ],
            fallback: false,
            language: Some("xml"),
        }
    }
}

impl Format for TextViewer {
    fn id(&self) -> FormatId {
        self.id
    }

    fn claims(&self, _name: &str, ext: Option<&str>, _head: &[u8]) -> Claim {
        if ext.is_some_and(|e| self.extensions.contains(&e)) {
            Claim::Strong
        } else if self.fallback {
            Claim::Weak
        } else {
            Claim::No
        }
    }

    fn render(&self, text: &str, cx: &RenderCx<'_>) -> Rendered {
        let language = self
            .language
            .map(str::to_owned)
            .or_else(|| ext_of(name_of(cx.path)));
        Rendered {
            body: Body::Text {
                text: text.to_owned(),
                language,
                problem: None,
                partial: None,
            },
            notes: Vec::new(),
        }
    }
}

/// Pictures, sound and video: extension, kind, MIME type. The webview plays what its codecs allow; a file it cannot
/// play is still listed so it is found and can be handed to the system's program.
pub const MEDIA_EXTENSIONS: &[(&str, MediaKind, &str)] = &[
    ("png", MediaKind::Image, "image/png"),
    ("jpg", MediaKind::Image, "image/jpeg"),
    ("jpeg", MediaKind::Image, "image/jpeg"),
    ("gif", MediaKind::Image, "image/gif"),
    ("webp", MediaKind::Image, "image/webp"),
    ("avif", MediaKind::Image, "image/avif"),
    ("bmp", MediaKind::Image, "image/bmp"),
    ("ico", MediaKind::Image, "image/x-icon"),
    ("svg", MediaKind::Image, "image/svg+xml"),
    ("mp3", MediaKind::Audio, "audio/mpeg"),
    ("wav", MediaKind::Audio, "audio/wav"),
    ("ogg", MediaKind::Audio, "audio/ogg"),
    ("oga", MediaKind::Audio, "audio/ogg"),
    ("opus", MediaKind::Audio, "audio/ogg"),
    ("flac", MediaKind::Audio, "audio/flac"),
    ("m4a", MediaKind::Audio, "audio/mp4"),
    ("aac", MediaKind::Audio, "audio/aac"),
    ("weba", MediaKind::Audio, "audio/webm"),
    ("wma", MediaKind::Audio, "audio/x-ms-wma"),
    ("aif", MediaKind::Audio, "audio/aiff"),
    ("aiff", MediaKind::Audio, "audio/aiff"),
    ("mka", MediaKind::Audio, "audio/x-matroska"),
    ("mp4", MediaKind::Video, "video/mp4"),
    ("m4v", MediaKind::Video, "video/mp4"),
    ("webm", MediaKind::Video, "video/webm"),
    ("ogv", MediaKind::Video, "video/ogg"),
    ("mov", MediaKind::Video, "video/quicktime"),
    ("mkv", MediaKind::Video, "video/x-matroska"),
    ("avi", MediaKind::Video, "video/x-msvideo"),
    ("wmv", MediaKind::Video, "video/x-ms-wmv"),
    ("flv", MediaKind::Video, "video/x-flv"),
    ("mpg", MediaKind::Video, "video/mpeg"),
    ("mpeg", MediaKind::Video, "video/mpeg"),
    ("3gp", MediaKind::Video, "video/3gpp"),
];

/// Kind and MIME type of a file name by its extension.
#[must_use]
pub fn media_of(name: &str) -> Option<(MediaKind, &'static str)> {
    let ext = ext_of(name_of(name))?;
    MEDIA_EXTENSIONS
        .iter()
        .find(|(e, _, _)| *e == ext)
        .map(|(_, kind, mime)| (*kind, *mime))
}

/// Media files: never read as text.
#[derive(Debug, Clone, Copy)]
pub struct MediaFormat;

impl Format for MediaFormat {
    fn id(&self) -> FormatId {
        FormatId::MEDIA
    }

    fn claims(&self, name: &str, _ext: Option<&str>, _head: &[u8]) -> Claim {
        if media_of(name).is_some() {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn reads_text(&self) -> bool {
        false
    }

    fn render(&self, _text: &str, cx: &RenderCx<'_>) -> Rendered {
        let body = match media_of(cx.path) {
            Some((media, mime)) => Body::Media {
                media,
                mime: mime.to_owned(),
            },
            None => Body::Notice {
                notice: super::Notice::NoViewer,
            },
        };
        Rendered {
            body,
            notes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_by_extension_only() {
        assert_eq!(media_of("a/b/Pic.PNG").map(|m| m.0), Some(MediaKind::Image));
        assert_eq!(media_of("clip.webm").map(|m| m.0), Some(MediaKind::Video));
        assert_eq!(media_of("song.flac").map(|m| m.1), Some("audio/flac"));
        assert!(media_of("png").is_none());
        assert!(media_of(".png").is_none());
        assert!(media_of("a.png.").is_none());
        assert!(media_of("notes.md").is_none());
        assert_eq!(MediaFormat.claims("x.mp3", Some("mp3"), b""), Claim::Strong);
        assert!(!MediaFormat.reads_text());
    }

    #[test]
    fn text_viewers_claim_their_extensions() {
        let json = TextViewer::json();
        assert_eq!(json.claims("a.json", Some("json"), b""), Claim::Strong);
        assert_eq!(json.claims("a.xml", Some("xml"), b""), Claim::No);
        assert_eq!(
            TextViewer::xml().claims("a.svg", Some("svg"), b""),
            Claim::Strong
        );
        assert_eq!(TextViewer::plain().claims("any", None, b""), Claim::Weak);
    }
}
