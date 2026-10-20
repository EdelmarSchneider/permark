//! Fuzzy matching of file paths for Quick open. Ranking: file-name prefix > name substring > name subsequence >
//! path substring > path subsequence; a query of several words that does not match as typed matches when each word
//! matches the path. Pure; characters are folded one to one (so positions point into the original path).

use unicode_normalization::UnicodeNormalization;

/// One matched path: higher score is better; `positions` are indices (characters) of the matched characters.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub score: f32,
    pub positions: Vec<u32>,
}

/// One character folded to one character: lowercase, accent gone (`É` -> `e`).
#[must_use]
pub fn fold_char(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let base = c.nfkd().next().unwrap_or(c);
    base.to_lowercase().next().unwrap_or(base)
}

/// The text with every character folded; same character count as the original.
#[must_use]
pub fn fold_aligned(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

/// `Some(folded)` when folding changes the text (the common all-lowercase ASCII path needs no copy).
#[must_use]
pub fn folded_if_different(s: &str) -> Option<Box<str>> {
    let folded = fold_aligned(s);
    (folded != s).then(|| folded.into_boxed_str())
}

fn boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '/' | '-' | '_' | '.')
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|x| x == c))
}

fn range(start: usize, len: usize) -> Vec<u32> {
    (start..start + len)
        .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
        .collect()
}

/// Leftmost subsequence match of `q` in `text[from..]` preferring nothing fancy (greedy), scored by runs and word
/// starts. `offset` shifts the positions back into the whole path.
#[allow(clippy::cast_precision_loss)]
fn subsequence(text: &[char], q: &[char], offset: usize) -> Option<Match> {
    let mut positions: Vec<usize> = Vec::with_capacity(q.len());
    let mut from = 0;
    for &ch in q {
        let at = from + text[from..].iter().position(|&c| c == ch)?;
        positions.push(at);
        from = at + 1;
    }
    let mut score = 0.0_f32;
    for (i, &pos) in positions.iter().enumerate() {
        if i > 0 && pos == positions[i - 1] + 1 {
            score += 3.0;
        }
        if pos == 0 || boundary(text[pos - 1]) {
            score += 2.0;
        }
    }
    if let (Some(first), Some(last)) = (positions.first(), positions.last()) {
        let gaps = last - first - (positions.len() - 1);
        score -= gaps as f32 * 0.1;
    }
    Some(Match {
        score,
        positions: positions
            .into_iter()
            .map(|p| u32::try_from(offset + p).unwrap_or(u32::MAX))
            .collect(),
    })
}

fn find(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// `q` (folded, one piece) against `p` (folded path).
#[allow(clippy::cast_precision_loss)]
fn match_one(q: &[char], p: &[char]) -> Option<Match> {
    let name_start = p.iter().rposition(|&c| c == '/').map_or(0, |i| i + 1);
    let name = &p[name_start..];
    if name.starts_with(q) {
        return Some(Match {
            score: 1000.0 - name.len() as f32 * 0.5,
            positions: range(name_start, q.len()),
        });
    }
    if let Some(at) = find(name, q) {
        return Some(Match {
            score: 800.0 - at as f32 * 4.0 - name.len() as f32 * 0.5,
            positions: range(name_start + at, q.len()),
        });
    }
    if let Some(m) = subsequence(name, q, name_start) {
        return Some(Match {
            score: 500.0 + m.score,
            positions: m.positions,
        });
    }
    if let Some(at) = find(p, q) {
        return Some(Match {
            score: 300.0 - at as f32 * 0.5,
            positions: range(at, q.len()),
        });
    }
    subsequence(p, q, 0).map(|m| Match {
        score: 100.0 + m.score,
        positions: m.positions,
    })
}

/// A prepared query: folded whole text and its words.
#[derive(Debug, Clone)]
pub struct Needle {
    whole: String,
    words: Vec<String>,
}

impl Needle {
    /// `None` for an empty (blank) query.
    #[must_use]
    pub fn new(query: &str) -> Option<Self> {
        let whole = fold_aligned(query.trim());
        if whole.is_empty() {
            return None;
        }
        let words = whole.split_whitespace().map(str::to_owned).collect();
        Some(Self { whole, words })
    }

    /// Cheap necessary condition (no allocation): every word is a subsequence of the folded path.
    fn may_match(&self, folded: &str) -> bool {
        is_subsequence(&self.whole, folded) || self.words.iter().all(|w| is_subsequence(w, folded))
    }

    /// Scores `folded` (the folded path); `None` when it does not match.
    #[must_use]
    pub fn matches(&self, folded: &str) -> Option<Match> {
        if !self.may_match(folded) {
            return None;
        }
        let p: Vec<char> = folded.chars().collect();
        let whole: Vec<char> = self.whole.chars().collect();
        if let Some(found) = match_one(&whole, &p) {
            return Some(found);
        }
        if self.words.len() < 2 {
            return None;
        }
        let mut score = 0.0_f32;
        let mut positions: Vec<u32> = Vec::new();
        for word in &self.words {
            let chars: Vec<char> = word.chars().collect();
            let found = match_one(&chars, &p)?;
            score += found.score;
            positions.extend(found.positions);
        }
        positions.sort_unstable();
        positions.dedup();
        // the mean keeps the scale of a single word
        #[allow(clippy::cast_precision_loss)]
        let mean = score / self.words.len() as f32;
        Some(Match {
            score: mean,
            positions,
        })
    }
}
