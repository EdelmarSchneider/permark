//! The tokenizers the person can choose, their ids, and the effective choice of a project.

use crate::bpe::Rules;

/// An exact encoding: its public rank file is downloaded once, after the person confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    O200k,
    Cl100k,
}

impl Encoding {
    pub const ALL: [Self; 2] = [Self::O200k, Self::Cl100k];

    /// Name of the file in the cache and at the source.
    #[must_use]
    pub fn file(self) -> &'static str {
        match self {
            Self::O200k => "o200k_base.tiktoken",
            Self::Cl100k => "cl100k_base.tiktoken",
        }
    }

    /// The official public address.
    #[must_use]
    pub fn url(self) -> &'static str {
        match self {
            Self::O200k => {
                "https://openaipublic.blob.core.windows.net/encodings/o200k_base.tiktoken"
            }
            Self::Cl100k => {
                "https://openaipublic.blob.core.windows.net/encodings/cl100k_base.tiktoken"
            }
        }
    }

    /// Size of the file, in bytes (pinned).
    #[must_use]
    pub fn bytes(self) -> u64 {
        match self {
            Self::O200k => 3_613_922,
            Self::Cl100k => 1_681_126,
        }
    }

    /// SHA-256 of the file, hex (pinned).
    #[must_use]
    pub fn sha256(self) -> &'static str {
        match self {
            Self::O200k => "446a9538cb6c348e3516120d7c08b09f57c36495e2acfffe59a5bf8b0cfb1a2d",
            Self::Cl100k => "223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7",
        }
    }

    #[must_use]
    pub fn rules(self) -> Rules {
        match self {
            Self::O200k => Rules::O200k,
            Self::Cl100k => Rules::Cl100k,
        }
    }

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::O200k => "o200k",
            Self::Cl100k => "cl100k",
        }
    }

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::O200k => 0,
            Self::Cl100k => 1,
        }
    }
}

/// What the person can choose to count with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tokenizer {
    /// Exact.
    Exact(Encoding),
    /// Estimate calibrated for Claude (the tokenizer is not public).
    Claude,
    /// Estimate calibrated for Gemini.
    Gemini,
    /// Characters divided by four.
    Chars4,
}

impl Tokenizer {
    pub const ALL: [Self; 5] = [
        Self::Exact(Encoding::O200k),
        Self::Exact(Encoding::Cl100k),
        Self::Claude,
        Self::Gemini,
        Self::Chars4,
    ];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Exact(encoding) => encoding.id(),
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Chars4 => "chars4",
        }
    }

    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    /// The encoding of an exact tokenizer.
    #[must_use]
    pub fn encoding(self) -> Option<Encoding> {
        match self {
            Self::Exact(encoding) => Some(encoding),
            Self::Claude | Self::Gemini | Self::Chars4 => None,
        }
    }
}

/// The tokenizers a project counts with: its own list when it names a known one, else the app's; unknown ids and
/// repeats dropped, at most three, first = primary.
#[must_use]
pub fn effective(app: &[String], project: &[String]) -> Vec<Tokenizer> {
    let known = |ids: &[String]| -> Vec<Tokenizer> {
        let mut out: Vec<Tokenizer> = Vec::new();
        for tokenizer in ids.iter().filter_map(|id| Tokenizer::parse(id)) {
            if !out.contains(&tokenizer) && out.len() < permark_core::settings::MAX_TOKENIZERS {
                out.push(tokenizer);
            }
        }
        out
    };
    let own = known(project);
    if own.is_empty() {
        known(app)
    } else {
        own
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]

    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names(list: &[Tokenizer]) -> Vec<&'static str> {
        list.iter().map(|t| t.id()).collect()
    }

    #[test]
    fn ids_round_trip() {
        for tokenizer in Tokenizer::ALL {
            assert_eq!(Tokenizer::parse(tokenizer.id()), Some(tokenizer));
        }
        assert_eq!(Tokenizer::parse("gpt"), None);
        assert_eq!(Tokenizer::parse(""), None);
    }

    #[test]
    fn the_project_choice_wins_when_it_has_a_known_id() {
        let app = ids(&["o200k"]);
        assert_eq!(names(&effective(&app, &ids(&["claude"]))), ["claude"]);
        assert_eq!(names(&effective(&app, &ids(&[]))), ["o200k"]);
        assert_eq!(names(&effective(&app, &ids(&["nope"]))), ["o200k"]);
        assert_eq!(
            names(&effective(&app, &ids(&["nope", "gemini"]))),
            ["gemini"]
        );
    }

    #[test]
    fn unknown_ids_repeats_and_extras_are_dropped() {
        let app = ids(&["x", "claude", "claude", "o200k", "gemini", "chars4"]);
        assert_eq!(names(&effective(&app, &[])), ["claude", "o200k", "gemini"]);
        assert!(effective(&[], &[]).is_empty());
    }

    #[test]
    fn the_pins_are_well_formed() {
        for encoding in Encoding::ALL {
            assert_eq!(encoding.sha256().len(), 64);
            assert!(encoding.sha256().bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(encoding
                .url()
                .starts_with("https://openaipublic.blob.core.windows.net/encodings/"));
            assert!(encoding.url().ends_with(encoding.file()));
            assert!(encoding.bytes() > 1_000_000);
        }
    }
}
