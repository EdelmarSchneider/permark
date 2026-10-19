//! Counting: a text by a tokenizer, with a small cache, and the sections of a text by their lines.

use std::sync::{Arc, Mutex, PoisonError};

use permark_core::plugins::AppServices;
use permark_core::settings::ProjectConfig;
use permark_core::tokens::{Counted, TokenCounter};
use xxhash_rust::xxh3::xxh3_128;

use crate::estimate::estimate;
use crate::tokenizer::{effective, Tokenizer};
use crate::vocab::{Unavailable, VocabStore};

/// Results kept by (text, tokenizer).
const CACHE_ENTRIES: usize = 64;
/// Texts shorter than this are cheaper to count again than to look up.
const CACHE_FROM_BYTES: usize = 2_000;

pub struct Counter {
    vocab: Arc<VocabStore>,
    /// Newest first.
    cache: Mutex<Vec<(u128, Tokenizer, u64)>>,
}

impl Counter {
    #[must_use]
    pub fn new(vocab: Arc<VocabStore>) -> Arc<Self> {
        Arc::new(Self {
            vocab,
            cache: Mutex::new(Vec::new()),
        })
    }

    /// Tokens of `text` by `tokenizer`; an exact tokenizer without its vocabulary on this computer is unavailable.
    ///
    /// # Errors
    /// `NoVocab`.
    pub fn count(&self, tokenizer: Tokenizer, text: &str) -> Result<u64, Unavailable> {
        let Some(encoding) = tokenizer.encoding() else {
            return Ok(estimate(tokenizer, text));
        };
        let key = (text.len() >= CACHE_FROM_BYTES).then(|| xxh3_128(text.as_bytes()));
        if let Some(key) = key {
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(at) = cache
                .iter()
                .position(|(k, t, _)| *k == key && *t == tokenizer)
            {
                let hit = cache.remove(at);
                cache.insert(0, hit);
                return Ok(hit.2);
            }
        }
        let value = self.vocab.load(encoding)?.count(text);
        if let Some(key) = key {
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            cache.insert(0, (key, tokenizer, value));
            cache.truncate(CACHE_ENTRIES);
        }
        Ok(value)
    }
}

/// The text of lines `from..=to` (1-based) of `text` split at `\n`, each with its line break (a `\r` stays): the text
/// a read of that part returns. Out-of-range lines are cut to the text; an empty range is empty.
#[must_use]
pub fn lines_of<'t>(text: &'t str, starts: &[usize], from: u32, to: u32) -> &'t str {
    let count = starts.len();
    let first = (from.max(1) as usize).min(count + 1);
    let last = (to as usize).min(count);
    if first > last {
        return "";
    }
    let begin = starts[first - 1];
    let end = if last == count {
        text.len()
    } else {
        starts[last]
    };
    &text[begin..end]
}

/// Byte offsets where each line of `text` starts.
#[must_use]
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    starts
}

/// What the agent connection asks: the primary tokenizer of a project over the app's choice.
pub struct CounterService {
    pub counter: Arc<Counter>,
    pub app: AppServices,
}

impl TokenCounter for CounterService {
    fn primary(&self, project: &ProjectConfig, text: &str) -> Option<Counted> {
        let chosen = effective(&self.app.settings().tokenizers, &project.tokenizers);
        let tokenizer = *chosen.first()?;
        let value = self.counter.count(tokenizer, text).ok()?;
        Some(Counted {
            value,
            tokenizer: tokenizer.id(),
            approx: tokenizer.encoding().is_none(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_cut_by_number() {
        let text = "one\ntwo\r\nthree\n\nfive";
        let starts = line_starts(text);
        assert_eq!(starts.len(), 5);
        assert_eq!(lines_of(text, &starts, 1, 1), "one\n");
        assert_eq!(lines_of(text, &starts, 2, 3), "two\r\nthree\n");
        assert_eq!(lines_of(text, &starts, 4, 4), "\n");
        assert_eq!(lines_of(text, &starts, 4, 5), "\nfive");
        assert_eq!(lines_of(text, &starts, 1, 5), text);
        // out of range: cut to the text
        assert_eq!(lines_of(text, &starts, 0, 2), "one\ntwo\r\n");
        assert_eq!(lines_of(text, &starts, 5, 99), "five");
        assert_eq!(lines_of(text, &starts, 6, 9), "");
        assert_eq!(lines_of(text, &starts, 3, 2), "");
        assert_eq!(lines_of("", &line_starts(""), 1, 1), "");
    }

    #[test]
    fn a_trailing_line_break_makes_an_empty_last_line() {
        let text = "a\nb\n";
        let starts = line_starts(text);
        assert_eq!(starts, [0, 2, 4]);
        assert_eq!(lines_of(text, &starts, 2, 3), "b\n");
        assert_eq!(lines_of(text, &starts, 2, 2), "b\n");
        assert_eq!(lines_of(text, &starts, 3, 3), "");
    }
}
