//! In-memory BM25 over document sections: terms interned to ids, postings per term id, section texts kept for
//! snippets. Insert-only builds append; removal is by batch (one pass over the affected posting lists).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::fold::{is_cjk, tokenize, Token, TokenKind};
use super::{SectionHit, SnippetPart};

const K1: f32 = 1.2;
const B: f32 = 0.75;
/// Terms that merely extend the last typed word count less than exact matches.
const PREFIX_WEIGHT: f32 = 0.6;
/// Upper bound on prefix expansions of the last query token.
const MAX_PREFIX_TERMS: usize = 64;
/// Extra score (as a factor) per additional distinct query token found in a section.
const COVERAGE_BONUS: f32 = 0.35;
/// Query tokens used (the rest is ignored).
const MAX_QUERY_TOKENS: usize = 16;
const SNIPPET_CHARS: usize = 220;
const SNIPPET_LEAD_CHARS: usize = 40;

/// A section ready to be inserted: its terms are counted (weights applied) by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInput {
    pub heading_path: Vec<String>,
    pub anchor: Option<String>,
    pub line: u32,
    pub text: String,
    /// `(term, weighted frequency)`.
    pub terms: Vec<(String, u32)>,
    /// Weighted token count.
    pub len: u32,
}

#[derive(Debug)]
struct Entry {
    path: Arc<str>,
    heading_path: Box<[Box<str>]>,
    anchor: Option<Box<str>>,
    line: u32,
    len: u32,
    text: Box<str>,
    terms: Box<[u32]>,
}

#[derive(Debug, Default)]
struct Terms {
    ids: HashMap<Arc<str>, u32>,
    /// Same keys in order: prefix expansion is a range scan.
    sorted: BTreeMap<Arc<str>, u32>,
    texts: Vec<Option<Arc<str>>>,
    postings: Vec<Vec<(u32, u32)>>,
    free: Vec<u32>,
}

impl Terms {
    fn intern(&mut self, term: &str) -> u32 {
        if let Some(&id) = self.ids.get(term) {
            return id;
        }
        let text: Arc<str> = Arc::from(term);
        let id = if let Some(id) = self.free.pop() {
            self.texts[id as usize] = Some(Arc::clone(&text));
            id
        } else {
            self.texts.push(Some(Arc::clone(&text)));
            self.postings.push(Vec::new());
            u32::try_from(self.texts.len() - 1).unwrap_or(u32::MAX)
        };
        self.ids.insert(Arc::clone(&text), id);
        self.sorted.insert(text, id);
        id
    }

    fn release_if_empty(&mut self, id: u32) {
        let slot = id as usize;
        if self.postings.get(slot).is_some_and(Vec::is_empty) {
            if let Some(text) = self.texts.get_mut(slot).and_then(Option::take) {
                self.ids.remove(&*text);
                self.sorted.remove(&*text);
                self.postings[slot] = Vec::new();
                self.free.push(id);
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Bm25 {
    terms: Terms,
    chunks: Vec<Option<Entry>>,
    free: Vec<u32>,
    live: usize,
    total_len: u64,
}

/// Which words of a text count as matches (snippets).
struct Matcher {
    terms: HashSet<String>,
    /// Folded text of the last query word: matches any word that starts with it.
    prefix: Option<String>,
}

impl Matcher {
    fn matches(&self, t: &Token) -> bool {
        self.terms.contains(&t.term)
            || self
                .prefix
                .as_deref()
                .is_some_and(|p| t.folded.starts_with(p))
    }
}

impl Bm25 {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of indexed sections.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Distinct terms in the index.
    #[must_use]
    pub fn term_count(&self) -> usize {
        self.terms.ids.len()
    }

    /// Adds the sections of one document; returns their ids.
    pub fn add(&mut self, path: &Arc<str>, sections: Vec<SectionInput>) -> Vec<u32> {
        let mut ids = Vec::with_capacity(sections.len());
        for section in sections {
            let id = self.allocate();
            let mut term_ids = Vec::with_capacity(section.terms.len());
            for (term, tf) in &section.terms {
                let tid = self.terms.intern(term);
                self.terms.postings[tid as usize].push((id, *tf));
                term_ids.push(tid);
            }
            self.total_len += u64::from(section.len);
            self.live += 1;
            self.chunks[id as usize] = Some(Entry {
                path: Arc::clone(path),
                heading_path: section
                    .heading_path
                    .into_iter()
                    .map(String::into_boxed_str)
                    .collect(),
                anchor: section.anchor.map(String::into_boxed_str),
                line: section.line,
                len: section.len,
                text: section.text.into_boxed_str(),
                terms: term_ids.into_boxed_slice(),
            });
            ids.push(id);
        }
        ids
    }

    fn allocate(&mut self) -> u32 {
        if let Some(id) = self.free.pop() {
            return id;
        }
        self.chunks.push(None);
        u32::try_from(self.chunks.len() - 1).unwrap_or(u32::MAX)
    }

    /// Removes sections by id (one pass over the posting lists they touch).
    pub fn remove(&mut self, ids: &[u32]) {
        let gone: HashSet<u32> = ids.iter().copied().collect();
        let mut touched: HashSet<u32> = HashSet::new();
        for &id in ids {
            let Some(entry) = self.chunks.get_mut(id as usize).and_then(Option::take) else {
                continue;
            };
            self.total_len = self.total_len.saturating_sub(u64::from(entry.len));
            self.live = self.live.saturating_sub(1);
            touched.extend(entry.terms.iter().copied());
            self.free.push(id);
        }
        for tid in touched {
            if let Some(list) = self.terms.postings.get_mut(tid as usize) {
                list.retain(|(chunk, _)| !gone.contains(chunk));
            }
            self.terms.release_if_empty(tid);
        }
    }

    #[allow(clippy::cast_precision_loss)] // counts stay far below 2^24
    fn idf(&self, df: usize) -> f32 {
        let n = self.live as f32;
        let df = df as f32;
        (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
    }

    /// Ranked sections for a free-text query; the last word also matches as a prefix. `allow`: which documents may
    /// answer (the sections of the others never compete for a place).
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // counts stay far below 2^24
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        allow: Option<&dyn Fn(&str) -> bool>,
    ) -> Vec<SectionHit> {
        // The index holds every CJK character and every pair; a query only needs the pairs (a lone character has no
        // pair, so it stays and matches the unigrams).
        let mut q: Vec<Token> = tokenize(query)
            .into_iter()
            .filter(|t| t.kind != TokenKind::CjkRunUnigram)
            .collect();
        q.truncate(MAX_QUERY_TOKENS);
        if q.is_empty() || self.live == 0 || limit == 0 {
            return Vec::new();
        }
        let avgdl = (self.total_len as f32 / self.live as f32).max(1.0);
        let last = q.len() - 1;
        let mut scores = vec![0.0_f32; self.chunks.len()];
        let mut seen = vec![0_u32; self.chunks.len()];
        let mut touched: Vec<u32> = Vec::new();
        let mut verdict: HashMap<Arc<str>, bool> = HashMap::new();
        let mut matcher = Matcher {
            terms: HashSet::new(),
            prefix: None,
        };

        for (i, tok) in q.iter().enumerate() {
            matcher.terms.insert(tok.term.clone());
            let is_last = i == last;
            if is_last && tok.kind == TokenKind::Word {
                matcher.prefix = Some(tok.folded.clone());
            }
            let candidates = self.candidates(tok, is_last);
            for (tid, weight) in candidates {
                let posts = &self.terms.postings[tid as usize];
                let term_idf = self.idf(posts.len());
                for &(id, tf) in posts {
                    let Some(entry) = &self.chunks[id as usize] else {
                        continue;
                    };
                    if let Some(allow) = allow {
                        let ok = if let Some(&ok) = verdict.get(&entry.path) {
                            ok
                        } else {
                            let ok = allow(&entry.path);
                            verdict.insert(Arc::clone(&entry.path), ok);
                            ok
                        };
                        if !ok {
                            continue;
                        }
                    }
                    let tf = tf as f32;
                    let norm =
                        tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * entry.len as f32 / avgdl));
                    let slot = id as usize;
                    if seen[slot] == 0 {
                        touched.push(id);
                    }
                    scores[slot] += weight * term_idf * norm;
                    seen[slot] |= 1 << i;
                }
            }
        }

        let mut ranked: Vec<(u32, f32)> = touched
            .into_iter()
            .map(|id| {
                let extra = seen[id as usize].count_ones().saturating_sub(1);
                (
                    id,
                    scores[id as usize] * (1.0 + COVERAGE_BONUS * extra as f32),
                )
            })
            .collect();
        let order = |a: &(u32, f32), b: &(u32, f32)| {
            b.1.total_cmp(&a.1).then_with(|| {
                match (&self.chunks[a.0 as usize], &self.chunks[b.0 as usize]) {
                    (Some(x), Some(y)) => (&x.path, x.line).cmp(&(&y.path, y.line)),
                    _ => a.0.cmp(&b.0),
                }
            })
        };
        if ranked.len() > limit {
            ranked.select_nth_unstable_by(limit, order);
            ranked.truncate(limit);
        }
        ranked.sort_by(order);

        self.hits_of(&ranked, &matcher)
    }

    /// The posting lists a query token reads: its own term, and for the last (Word) token the terms it prefixes.
    /// CJK terms have no longer forms to complete: every character is already indexed alone.
    fn candidates(&self, tok: &Token, is_last: bool) -> Vec<(u32, f32)> {
        let mut candidates: Vec<(u32, f32)> = Vec::new();
        if let Some(&id) = self.terms.ids.get(tok.term.as_str()) {
            candidates.push((id, 1.0));
        }
        if is_last && tok.kind == TokenKind::Word {
            candidates.extend(
                self.terms
                    .sorted
                    .range::<str, _>((
                        std::ops::Bound::Included(tok.folded.as_str()),
                        std::ops::Bound::Unbounded,
                    ))
                    .take_while(|(t, _)| t.starts_with(tok.folded.as_str()))
                    .filter(|(t, _)| ***t != *tok.term)
                    .take(MAX_PREFIX_TERMS)
                    .map(|(_, &id)| (id, PREFIX_WEIGHT)),
            );
        }
        candidates
    }

    fn hits_of(&self, ranked: &[(u32, f32)], matcher: &Matcher) -> Vec<SectionHit> {
        ranked
            .iter()
            .filter_map(|&(id, score)| {
                let e = self.chunks[id as usize].as_ref()?;
                let heading = e.heading_path.last().map_or("", |h| &**h);
                Some(SectionHit {
                    path: e.path.to_string(),
                    heading_path: e.heading_path.iter().map(ToString::to_string).collect(),
                    anchor: e.anchor.as_ref().map(ToString::to_string),
                    line: e.line,
                    snippet: snippet(&e.text, heading, matcher),
                    score,
                })
            })
            .collect()
    }
}

/// Excerpt of `text` around the first match, matches marked; the heading text when the body has no match (the
/// section matched by its title). Secret values are hidden before the cut: an excerpt that starts between a key and
/// its value never shows the value without its key.
fn snippet(text: &str, heading: &str, matcher: &Matcher) -> Vec<SnippetPart> {
    let text = crate::secrets::mask_text(text);
    let heading = crate::secrets::mask_text(heading);
    let (text, heading) = (text.as_ref(), heading.as_ref());
    let body = tokenize(text);
    let (source, toks) = if body.iter().any(|t| matcher.matches(t)) || heading.is_empty() {
        (text, body)
    } else {
        (heading, tokenize(heading))
    };
    let first = toks.iter().position(|t| matcher.matches(t));
    let start = first.map_or(0, |i| lead_start(source, toks[i].start));
    let mut end = start;
    for (taken, (i, c)) in source[start..].char_indices().enumerate() {
        if taken == SNIPPET_CHARS {
            break;
        }
        end = start + i + c.len_utf8();
    }
    // do not cut the last word in half
    if end < source.len() && splits_a_word(source, end) {
        if let Some(sp) = source[start..end].rfind(char::is_whitespace) {
            if sp > 0 {
                end = start + sp;
            }
        }
    }

    // matched tokens can overlap (CJK pairs and characters) or touch: one mark per stretch
    let mut marks: Vec<(usize, usize)> = Vec::new();
    for t in toks
        .iter()
        .filter(|t| t.start >= start && t.end <= end && matcher.matches(t))
    {
        match marks.last_mut() {
            Some((_, e)) if t.start <= *e => *e = (*e).max(t.end),
            _ => marks.push((t.start, t.end)),
        }
    }

    let mut parts: Vec<SnippetPart> = Vec::new();
    let mut push = |text: String, mark: bool| {
        if !text.is_empty() {
            parts.push(SnippetPart { text, mark });
        }
    };
    let mut lead = String::new();
    if start > 0 {
        lead.push('\u{2026}');
    }
    let mut cursor = start;
    for (s, e) in marks {
        lead.push_str(&collapse(&source[cursor..s]));
        push(std::mem::take(&mut lead), false);
        push(source[s..e].to_owned(), true);
        cursor = e;
    }
    lead.push_str(&collapse(&source[cursor..end]));
    if end < source.len() {
        lead.push('\u{2026}');
    }
    push(lead, false);
    parts
}

/// Where the excerpt starts: a few characters before the match, never inside a spaced word.
fn lead_start(source: &str, match_start: usize) -> usize {
    let mut start = match_start;
    for (taken, (i, _)) in source[..match_start].char_indices().rev().enumerate() {
        if taken == SNIPPET_LEAD_CHARS {
            break;
        }
        start = i;
    }
    if start > 0 && start < match_start && splits_a_word(source, start) {
        // move on to the next word start
        if let Some(sp) = source[start..match_start].find(char::is_whitespace) {
            start += sp;
            start += source[start..].chars().next().map_or(0, char::len_utf8);
        }
    }
    start.min(match_start)
}

/// True when `at` falls inside a word of a spaced script (CJK has no words to protect from a cut).
fn splits_a_word(s: &str, at: usize) -> bool {
    let before = s[..at].chars().next_back();
    let after = s[at..].chars().next();
    matches!((before, after), (Some(b), Some(a))
        if b.is_alphanumeric() && a.is_alphanumeric() && !is_cjk(b) && !is_cjk(a))
}

/// Newlines and runs of spaces become single spaces.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}
