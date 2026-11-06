//! Prompt formats = catalog DATA (D-9; crate rule "NEVER hardcoded per model"): chat markup ([`PromptProfile`],
//! catalog `promptProfiles`) + embedding task prefixes ([`Prefixes`], catalog `prefixProfiles`). Engines render them.
//!
//! - [`PromptProfile::render`] -> [`Piece`]s: markup (special tokens parsed) | content (person's text: special
//!   tokens NEVER parsed -> no turn-marker injection).
//! - BOS NOT in the data: the tokenizer adds it (GGUF `tokenizer.ggml.add_bos_token`).
//! - [`Role::Tool`] -> `Unsupported` (tool calls off in v1).
//! - [`StopScan`]: streamed answer cut before the first stop string; text that may start one held back.

use serde::{Deserialize, Serialize};

use crate::{ChatMessage, EmbedRole, EngineError, Role};

/// Markup around one message of a role.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Turn {
    pub prefix: String,
    pub suffix: String,
}

/// Chat markup of a model family.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptProfile {
    pub system: Turn,
    pub user: Turn,
    pub assistant: Turn,
    /// Opens the answer turn after the last message.
    pub generation: String,
    /// Answer ends before the first of these (end-of-generation tokens end it anyway).
    #[serde(default)]
    pub stop: Vec<String>,
    /// Message text trimmed first (template `| trim`).
    #[serde(default)]
    pub trim: bool,
}

/// One span of a rendered prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece<'a> {
    pub text: &'a str,
    /// Profile markup (special tokens parsed) | `false`: message text.
    pub markup: bool,
}

impl PromptProfile {
    /// `messages` as prompt spans, answer opener last.
    ///
    /// # Errors
    /// [`EngineError::Unsupported`]: a [`Role::Tool`] message.
    pub fn render<'a>(
        &'a self,
        messages: &'a [ChatMessage],
    ) -> Result<Vec<Piece<'a>>, EngineError> {
        fn push<'a>(pieces: &mut Vec<Piece<'a>>, text: &'a str, markup: bool) {
            if !text.is_empty() {
                pieces.push(Piece { text, markup });
            }
        }
        let mut pieces = Vec::with_capacity(messages.len() * 3 + 1);
        for message in messages {
            let turn = match message.role {
                Role::System => &self.system,
                Role::User => &self.user,
                Role::Assistant => &self.assistant,
                Role::Tool => {
                    return Err(EngineError::Unsupported {
                        what: "tool results".into(),
                    })
                }
            };
            let content = if self.trim {
                message.content.trim()
            } else {
                message.content.as_str()
            };
            push(&mut pieces, &turn.prefix, true);
            push(&mut pieces, content, false);
            push(&mut pieces, &turn.suffix, true);
        }
        push(&mut pieces, &self.generation, true);
        Ok(pieces)
    }
}

/// Embedding task prefixes of a model family (`EmbeddingGemma`: `task: search result | query: `).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Prefixes {
    pub query: String,
    pub document: String,
    /// Similarity | clustering (same prefix both sides).
    pub symmetric: String,
}

impl Prefixes {
    #[must_use]
    pub fn of(&self, role: EmbedRole) -> &str {
        match role {
            EmbedRole::Query => &self.query,
            EmbedRole::Document => &self.document,
            EmbedRole::Symmetric => &self.symmetric,
        }
    }
}

/// Stop-string cut of a streamed answer.
#[derive(Debug, Clone, Default)]
pub struct StopScan {
    stops: Vec<String>,
    held: String,
}

impl StopScan {
    /// Over `stops` (empty ones ignored).
    #[must_use]
    pub fn new<'a>(stops: impl IntoIterator<Item = &'a String>) -> Self {
        Self {
            stops: stops
                .into_iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect(),
            held: String::new(),
        }
    }

    /// Adds `text`; returns what may go out now + `true` when a stop string was reached (answer ends; rest dropped).
    pub fn push(&mut self, text: &str) -> (String, bool) {
        self.held.push_str(text);
        let first = self
            .stops
            .iter()
            .filter_map(|stop| self.held.find(stop.as_str()))
            .min();
        if let Some(at) = first {
            let out = self.held[..at].to_owned();
            self.held.clear();
            return (out, true);
        }
        // longest tail that is the start of a stop string stays held
        let keep = self
            .stops
            .iter()
            .flat_map(|stop| {
                (1..stop.len())
                    .rev()
                    .filter(|&n| stop.is_char_boundary(n) && self.held.ends_with(&stop[..n]))
                    .take(1)
            })
            .max()
            .unwrap_or(0);
        let cut = self.held.len() - keep;
        let out = self.held[..cut].to_owned();
        self.held.drain(..cut);
        (out, false)
    }

    /// Held text when the answer ends without a stop string.
    #[must_use]
    pub fn finish(self) -> String {
        self.held
    }
}
