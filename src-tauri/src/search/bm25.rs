//! In-memory BM25 index over document sections.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::chunk::Chunk;
use super::fold::{is_cjk, tokenize, Token, TokenKind};
use super::{SearchHit, SearchProvider};
use crate::util::escape_html;
use crate::vault::key;

const K1: f32 = 1.2;
const B: f32 = 0.75;
/// A hit on the section's own heading counts this many times in term frequency.
const HEADING_WEIGHT: u32 = 3;
/// Terms that merely extend the last typed word count less than exact matches.
const PREFIX_WEIGHT: f32 = 0.6;
/// Upper bound on prefix expansions for the last query term.
const MAX_PREFIX_TERMS: usize = 64;
/// Extra score per additional distinct query term found in a section.
const COVERAGE_BONUS: f32 = 0.35;
const SNIPPET_CHARS: usize = 220;
const SNIPPET_LEAD_WORDS: usize = 6;

#[derive(Debug)]
struct ChunkEntry {
    path: String,
    heading_path: Vec<String>,
    start_line: u32,
    text: String,
    /// Weighted number of indexed terms.
    len: u32,
    /// Distinct terms, to find the posting lists on removal.
    terms: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Bm25Index {
    chunks: Vec<Option<ChunkEntry>>,
    free: Vec<u32>,
    /// term to (chunk id, term frequency); a `BTreeMap` so prefixes are a range scan.
    postings: BTreeMap<String, Vec<(u32, u32)>>,
    docs: HashMap<String, Vec<u32>>,
    total_len: u64,
    live: usize,
}

/// Which words of a text count as matches (for snippets).
struct Matcher {
    terms: HashSet<String>,
    /// Folded text of the last query word, matching any word that starts with it.
    prefix: Option<String>,
}

impl Matcher {
    fn matches(&self, t: &Token) -> bool {
        self.terms.contains(&t.term)
            || self
                .prefix
                .as_ref()
                .is_some_and(|p| t.folded.starts_with(p))
    }
}

impl Bm25Index {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of indexed sections.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.live
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Indexes (or re-indexes) a Markdown document under its vault-relative `path`.
    #[cfg(test)]
    pub fn add_doc(&mut self, path: &str, markdown: &str) {
        self.add_chunks(path, super::chunk::chunk_markdown(markdown));
    }

    pub fn add_chunks(&mut self, path: &str, chunks: Vec<Chunk>) {
        self.remove_doc(path);
        let doc_key = key(path);
        let mut ids = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let id = self.allocate();
            let mut tf: HashMap<String, u32> = HashMap::new();
            for t in tokenize(&chunk.text) {
                *tf.entry(t.term).or_default() += 1;
            }
            for (depth, heading) in chunk.heading_path.iter().enumerate() {
                let own = depth + 1 == chunk.heading_path.len();
                for t in tokenize(heading) {
                    *tf.entry(t.term).or_default() += if own { HEADING_WEIGHT } else { 1 };
                }
            }
            let len: u32 = tf.values().sum();
            let mut terms = Vec::with_capacity(tf.len());
            for (term, freq) in tf {
                self.postings
                    .entry(term.clone())
                    .or_default()
                    .push((id, freq));
                terms.push(term);
            }
            self.total_len += u64::from(len);
            self.live += 1;
            self.chunks[id as usize] = Some(ChunkEntry {
                path: path.to_owned(),
                heading_path: chunk.heading_path,
                start_line: chunk.start_line,
                text: chunk.text,
                len,
                terms,
            });
            ids.push(id);
        }
        if !ids.is_empty() {
            self.docs.insert(doc_key, ids);
        }
    }

    fn allocate(&mut self) -> u32 {
        if let Some(id) = self.free.pop() {
            return id;
        }
        self.chunks.push(None);
        u32::try_from(self.chunks.len() - 1).unwrap_or(u32::MAX)
    }

    /// Removes every document below the folder `dir` (`docs` removes `docs/a.md` and `docs/x/b.md`,
    /// not `docs2/c.md`). Returns how many documents went.
    pub fn remove_under(&mut self, dir: &str) -> usize {
        let prefix = format!("{}/", key(dir).trim_end_matches('/'));
        let below: Vec<String> = self
            .docs
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for k in &below {
            self.remove_doc(k); // keys are already folded; folding again changes nothing
        }
        below.len()
    }

    /// Removes a document from the index; unknown paths are ignored.
    pub fn remove_doc(&mut self, path: &str) {
        let Some(ids) = self.docs.remove(&key(path)) else {
            return;
        };
        for id in ids {
            let Some(entry) = self.chunks[id as usize].take() else {
                continue;
            };
            for term in &entry.terms {
                if let Some(list) = self.postings.get_mut(term) {
                    list.retain(|(c, _)| *c != id);
                    if list.is_empty() {
                        self.postings.remove(term);
                    }
                }
            }
            self.total_len -= u64::from(entry.len);
            self.live -= 1;
            self.free.push(id);
        }
    }

    #[allow(clippy::cast_precision_loss)] // counts stay far below 2^24
    fn idf(&self, df: usize) -> f32 {
        let n = self.live as f32;
        let df = df as f32;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Ranked sections for a free-text query. The last word also matches as a prefix (search-as-you-type).
    #[allow(clippy::cast_precision_loss)] // counts stay far below 2^24
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        // The index holds every CJK character and every pair; a query only needs the pairs (a single
        // character has no pair, so it stays and matches the unigrams).
        let q: Vec<Token> = tokenize(query)
            .into_iter()
            .filter(|t| t.kind != TokenKind::CjkRunUnigram)
            .collect();
        if q.is_empty() || self.live == 0 || limit == 0 {
            return Vec::new();
        }
        let avgdl = (self.total_len as f32 / self.live as f32).max(1.0);
        let last = q.len() - 1;
        let mut scores: HashMap<u32, f32> = HashMap::new();
        let mut seen_terms: HashMap<u32, HashSet<usize>> = HashMap::new();
        let mut matcher = Matcher {
            terms: HashSet::new(),
            prefix: None,
        };

        for (i, tok) in q.iter().enumerate() {
            matcher.terms.insert(tok.term.clone());
            let mut candidates: Vec<(&str, f32)> = vec![(tok.term.as_str(), 1.0)];
            // CJK terms have no longer forms to complete: every character is already indexed alone.
            if i == last && tok.kind == TokenKind::Word {
                matcher.prefix = Some(tok.folded.clone());
                candidates.extend(
                    self.postings
                        .range(tok.folded.clone()..)
                        .take_while(|(t, _)| t.starts_with(&tok.folded))
                        .filter(|(t, _)| **t != tok.term)
                        .take(MAX_PREFIX_TERMS)
                        .map(|(t, _)| (t.as_str(), PREFIX_WEIGHT)),
                );
            }
            for (term, weight) in candidates {
                let Some(posts) = self.postings.get(term) else {
                    continue;
                };
                let term_idf = self.idf(posts.len());
                for &(id, tf) in posts {
                    let Some(entry) = &self.chunks[id as usize] else {
                        continue;
                    };
                    let tf = tf as f32;
                    let norm =
                        tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * entry.len as f32 / avgdl));
                    *scores.entry(id).or_default() += weight * term_idf * norm;
                    seen_terms.entry(id).or_default().insert(i);
                }
            }
        }

        let mut ranked: Vec<(u32, f32)> = scores
            .into_iter()
            .map(|(id, s)| {
                let extra = seen_terms
                    .get(&id)
                    .map_or(0, HashSet::len)
                    .saturating_sub(1);
                (id, s * (1.0 + COVERAGE_BONUS * extra as f32))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.1.total_cmp(&a.1).then_with(|| {
                let (ea, eb) = (&self.chunks[a.0 as usize], &self.chunks[b.0 as usize]);
                match (ea, eb) {
                    (Some(x), Some(y)) => (&x.path, x.start_line).cmp(&(&y.path, y.start_line)),
                    _ => a.0.cmp(&b.0),
                }
            })
        });
        ranked.truncate(limit);

        ranked
            .into_iter()
            .filter_map(|(id, score)| {
                let e = self.chunks[id as usize].as_ref()?;
                Some(SearchHit {
                    path: e.path.clone(),
                    heading_path: e.heading_path.clone(),
                    snippet: snippet(&e.text, &e.heading_path, &matcher),
                    start_line: e.start_line,
                    score,
                })
            })
            .collect()
    }

    /// Paths of all indexed documents.
    pub fn paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .docs
            .values()
            .filter_map(|ids| ids.first())
            .filter_map(|id| self.chunks[*id as usize].as_ref().map(|e| e.path.clone()))
            .collect();
        v.sort();
        v
    }
}

impl SearchProvider for Bm25Index {
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        Bm25Index::search(self, query, limit)
    }
}

/// HTML-escaped excerpt of `text` around the first match, with matches wrapped in `<mark>`.
/// Falls back to the heading text when the body has no match (the section matched by its title).
fn snippet(text: &str, heading_path: &[String], matcher: &Matcher) -> String {
    let (source, toks) = {
        let toks = tokenize(text);
        if toks.iter().any(|t| matcher.matches(t)) || heading_path.is_empty() {
            (text.to_owned(), toks)
        } else {
            let heading = heading_path.last().cloned().unwrap_or_default();
            let toks = tokenize(&heading);
            (heading, toks)
        }
    };
    let first = toks.iter().position(|t| matcher.matches(t));
    let mut start = first.map_or(0, |i| lead_start(&toks, i));
    if first.is_none() {
        start = 0;
    }
    let mut end = source.len().min(start + SNIPPET_CHARS);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    // Do not cut the last word in half.
    if end < source.len() && splits_a_word(&source, end) {
        if let Some(sp) = source[start..end].rfind(char::is_whitespace) {
            if sp > 0 {
                end = start + sp;
            }
        }
    }

    // Matched tokens can overlap (CJK pairs and characters) or touch: one mark per stretch.
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

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    let mut cursor = start;
    for (s, e) in marks {
        out.push_str(&escape_html(&collapse(&source[cursor..s])));
        out.push_str("<mark>");
        out.push_str(&escape_html(&source[s..e]));
        out.push_str("</mark>");
        cursor = e;
    }
    out.push_str(&escape_html(&collapse(&source[cursor..end])));
    if end < source.len() {
        out.push('…');
    }
    out
}

/// Where the excerpt starts: a few words before the match `toks[first]`. A CJK character counts as a
/// word; its duplicate unigram does not.
fn lead_start(toks: &[Token], first: usize) -> usize {
    let mut i = first;
    let mut words = 0;
    while i > 0 && words < SNIPPET_LEAD_WORDS {
        i -= 1;
        if toks[i].kind != TokenKind::CjkRunUnigram {
            words += 1;
        }
    }
    toks[i].start
}

/// True when `at` falls inside a word of a spaced script. CJK text has no words to protect from a cut,
/// and falling back to a far-away space there would throw the match out of the excerpt.
fn splits_a_word(s: &str, at: usize) -> bool {
    let in_word = |c: char| c.is_alphanumeric() && !is_cjk(c);
    s[..at].chars().next_back().is_some_and(in_word) && s[at..].chars().next().is_some_and(in_word)
}

/// Newlines and runs of spaces become single spaces.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHARACTERS: &str = "# Cast\n\n## Character descriptions\n\nMira is a pilot with a scar over her left eye. Joss is the engineer.\n\n## Locations\n\nThe station orbits a gas giant.\n";
    const PLOT: &str = "# Plot\n\n## Act one\n\nThe crew describes the character arcs briefly.\n\n## Act two\n\nA twist.\n";
    const RECIPES: &str = "# Recipes\n\n## Soup\n\nBoil water. Add salt.\n";

    fn index() -> Bm25Index {
        let mut i = Bm25Index::new();
        i.add_doc("story/cast.md", CHARACTERS);
        i.add_doc("story/plot.md", PLOT);
        i.add_doc("food.md", RECIPES);
        i
    }

    #[test]
    fn character_descriptions_finds_the_exact_section_first() {
        let hits = index().search("character descriptions", 5);
        assert_ne!(hits.len(), 0);
        let top = &hits[0];
        assert_eq!(top.path, "story/cast.md");
        assert_eq!(top.heading_path, ["Cast", "Character descriptions"]);
        assert_eq!(top.start_line, 3);
        assert!(top.snippet.contains("<mark>"), "{}", top.snippet);
        // The plot section mentions both words in a different form and ranks lower.
        assert!(hits.iter().any(|h| h.path == "story/plot.md"));
        assert!(hits.iter().position(|h| h.path == "story/plot.md") > Some(0));
    }

    #[test]
    fn matching_is_accent_case_and_stem_insensitive() {
        let mut i = Bm25Index::new();
        i.add_doc(
            "pt.md",
            "# Personagens\n\n## Descrição\n\nA Mira é piloto.\n",
        );
        for q in [
            "descricao",
            "DESCRIÇÃO",
            "descrições",
            "personagem",
            "PERSONAGENS",
        ] {
            let hits = i.search(q, 3);
            assert!(!hits.is_empty(), "query {q}");
        }
        let mut en = Bm25Index::new();
        en.add_doc("en.md", "# Notes\n\nThe heroes described their journeys.\n");
        assert_eq!(en.search("describe journey hero", 3).len(), 1);
    }

    #[test]
    fn last_term_matches_as_a_prefix() {
        let i = index();
        let hits = i.search("charac descr", 5);
        assert_eq!(hits[0].path, "story/cast.md", "{hits:?}");
        let hits = i.search("stat", 5);
        assert_eq!(hits[0].path, "story/cast.md");
        assert_eq!(i.search("zzzz", 5).len(), 0);
    }

    #[test]
    fn heading_text_alone_can_match_and_is_boosted() {
        let mut i = Bm25Index::new();
        i.add_doc("a.md", "# Alpha\n\n## Zebra\n\nunrelated words here\n");
        i.add_doc(
            "b.md",
            "# Beta\n\nA zebra appears once in a long paragraph with many other words around it.\n",
        );
        let hits = i.search("zebra", 5);
        assert_eq!(hits[0].path, "a.md");
        // The section matched by title shows the heading as its snippet.
        assert!(
            hits[0].snippet.contains("<mark>Zebra</mark>"),
            "{}",
            hits[0].snippet
        );
    }

    #[test]
    fn re_adding_replaces_and_removing_forgets() {
        let mut i = index();
        let before = i.len();
        i.add_doc("food.md", "# Recipes\n\n## Stew\n\nSimmer beef.\n");
        assert_eq!(i.len(), before);
        assert_eq!(i.search("boil", 5).len(), 0);
        assert_eq!(i.search("simmer", 5)[0].path, "food.md");
        i.remove_doc("food.md");
        assert_eq!(i.search("simmer", 5).len(), 0);
        assert!(!i.paths().contains(&"food.md".to_owned()));
        i.remove_doc("never-added.md");
    }

    #[test]
    fn removal_cleans_postings_and_reuses_slots() {
        let mut i = index();
        let chunks_slots = i.chunks.len();
        i.remove_doc("story/plot.md");
        i.remove_doc("story/cast.md");
        i.remove_doc("food.md");
        assert!(i.is_empty());
        assert!(i.postings.is_empty());
        assert_eq!(i.total_len, 0);
        i.add_doc("again.md", CHARACTERS);
        assert!(i.chunks.len() <= chunks_slots, "freed slots must be reused");
        assert_eq!(i.search("scar", 3)[0].path, "again.md");
    }

    #[test]
    fn snippets_are_html_escaped_and_marked() {
        let mut i = Bm25Index::new();
        i.add_doc(
            "x.md",
            "# T\n\nUse <script>alert('x')</script> & friends: the widget is here.\n",
        );
        let hits = i.search("widget", 3);
        let s = &hits[0].snippet;
        assert!(s.contains("<mark>widget</mark>"), "{s}");
        assert!(!s.contains("<script>"), "{s}");
        assert!(s.contains("&amp;"), "{s}");
    }

    #[test]
    fn snippets_are_windowed_around_the_first_match() {
        let filler = "lorem ipsum dolor sit amet ".repeat(40);
        let mut i = Bm25Index::new();
        i.add_doc("long.md", &format!("# T\n\n{filler} needle {filler}\n"));
        let s = &i.search("needle", 1)[0].snippet;
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(s.contains("<mark>needle</mark>"));
        assert!(s.chars().count() < 320, "{}", s.chars().count());
    }

    #[test]
    fn limit_and_empty_queries() {
        let i = index();
        assert_eq!(i.search("the", 1).len(), 1);
        assert_eq!(i.search("", 5).len(), 0);
        assert_eq!(i.search("   ", 5).len(), 0);
        assert_eq!(i.search("!!!", 5).len(), 0);
        assert_eq!(i.search("scar", 0).len(), 0);
        assert_eq!(Bm25Index::new().search("scar", 5).len(), 0);
    }

    #[test]
    fn multi_term_coverage_beats_a_single_frequent_term() {
        let mut i = Bm25Index::new();
        i.add_doc("one.md", "# A\n\nmira mira mira mira mira mira\n");
        i.add_doc("both.md", "# B\n\nmira flies the ship\n");
        let hits = i.search("mira ship", 3);
        assert_eq!(hits[0].path, "both.md");
    }

    #[test]
    fn remove_under_drops_a_whole_folder_but_not_its_namesakes() {
        let mut i = Bm25Index::new();
        for p in ["docs/a.md", "docs/x/b.md", "docs2/c.md", "docs.md"] {
            i.add_doc(p, "# T\n\nword\n");
        }
        assert_eq!(i.remove_under("docs"), 2);
        assert_eq!(i.paths(), ["docs.md", "docs2/c.md"]);
        assert_eq!(i.remove_under("docs/"), 0);
        assert_eq!(i.search("word", 5).len(), 2);
    }

    #[test]
    fn provider_trait_is_object_safe() {
        let i = index();
        let p: &dyn SearchProvider = &i;
        assert_ne!(p.search("pilot", 3).len(), 0);
    }

    #[test]
    fn unicode_documents_do_not_panic_in_snippets() {
        let mut i = Bm25Index::new();
        i.add_doc(
            "u.md",
            &format!("# 日本語\n\n{} 検索テスト emoji 🎉 end\n", "あ".repeat(300)),
        );
        let _ = i.search("検索テスト", 3);
        let _ = i.search("emoji", 3);
    }

    const ZH_FULL: &str =
        "# 笔记\n\n## 引擎\n\n这是一个用来检查搜索测试是否可行的很长的句子，没有任何空格。\n";
    const ZH_PART: &str = "# 笔记\n\n## 其他\n\n我们昨天讨论了搜索功能，还有别的内容。\n";
    const ZH_OTHER: &str = "# 食物\n\n## 汤\n\n煮水，加盐，然后喝汤。\n";

    fn chinese_index() -> Bm25Index {
        let mut i = Bm25Index::new();
        i.add_doc("zh/full.md", ZH_FULL);
        i.add_doc("zh/part.md", ZH_PART);
        i.add_doc("zh/other.md", ZH_OTHER);
        i
    }

    #[test]
    fn a_two_character_word_is_found_inside_a_long_sentence() {
        let i = chinese_index();
        let hits = i.search("空格", 5);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].path, "zh/full.md");
        let hits = i.search("搜索", 5);
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths.contains(&"zh/full.md") && paths.contains(&"zh/part.md"));
        // Characters that are not adjacent in the text are not a word.
        assert_eq!(i.search("搜测", 5).len(), 0);
        assert_eq!(i.search("苹果", 5).len(), 0);
    }

    #[test]
    fn the_section_holding_the_whole_chinese_query_ranks_first() {
        let hits = chinese_index().search("搜索测试", 5);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].path, "zh/full.md");
        assert_eq!(hits[1].path, "zh/part.md");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn a_single_character_finds_it_inside_a_longer_run() {
        let i = chinese_index();
        // 句 only occurs inside 一个用来…的句子.
        let hits = i.search("句", 5);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].path, "zh/full.md");
        assert!(hits[0].snippet.contains("<mark>句</mark>"));
        // 汤 is a run of one character in 然后喝汤 and a heading: both count.
        assert_eq!(i.search("汤", 5)[0].path, "zh/other.md");
        // A character that is part of several words matches all of them.
        assert_eq!(i.search("索", 5).len(), 2);
    }

    #[test]
    fn japanese_words_are_found_in_kana_and_kanji_text() {
        let mut i = Bm25Index::new();
        i.add_doc("ja.md", "# メモ\n\nこれは日本語の検索テストです。\n");
        i.add_doc("other.md", "# メモ\n\nきょうはいい天気です。\n");
        for q in ["検索", "テスト", "検索テスト", "日本語", "テ"] {
            let hits = i.search(q, 3);
            assert_eq!(hits[0].path, "ja.md", "query {q}");
        }
        assert_eq!(i.search("検索", 3).len(), 1);
        assert_eq!(i.search("天気", 3)[0].path, "other.md");
    }

    #[test]
    fn mixed_cjk_and_latin_words_match_separately() {
        let mut i = Bm25Index::new();
        i.add_doc("m.md", "# T\n\n使用Rust编写的 tooling 工具\n");
        for q in ["rust", "使用", "编写", "RUST 编写", "使用Rust"] {
            assert_eq!(i.search(q, 3).len(), 1, "query {q}");
        }
        // The last Latin word still matches as a prefix, also after CJK words.
        assert_eq!(i.search("编写 tool", 3).len(), 1);
        assert_eq!(i.search("rus", 3).len(), 1);
        assert_eq!(i.search("工具 zzz", 3).len(), 1);
    }

    #[test]
    fn chinese_snippets_mark_matches_and_merge_overlaps() {
        let i = chinese_index();
        // The three bigrams of the query overlap: one mark covers them all.
        let s = &i.search("搜索测试", 1)[0].snippet;
        assert!(s.contains("<mark>搜索测试</mark>"), "{s}");
        assert_eq!(s.matches("<mark>").count(), 1, "{s}");
        // A single bigram marks just itself.
        let s = &i.search("空格", 1)[0].snippet;
        assert!(s.contains("<mark>空格</mark>"), "{s}");
        // Adjacent single characters merge as well.
        let s = &i.search("搜 索", 1)[0].snippet;
        assert_eq!(s.matches("<mark>").count(), 1, "{s}");
    }

    #[test]
    fn long_runs_of_matching_characters_make_one_mark_without_panicking() {
        let mut i = Bm25Index::new();
        i.add_doc("a.md", &format!("# T\n\n{}。结束\n", "あ".repeat(300)));
        let s = &i.search("あ", 1)[0].snippet;
        assert_eq!(s.matches("<mark>").count(), 1, "{s}");
        assert!(s.ends_with('…'), "{s}");
        let s = &i.search("ああ", 1)[0].snippet;
        assert_eq!(s.matches("<mark>").count(), 1, "{s}");
    }

    #[test]
    fn a_far_space_does_not_shrink_a_chinese_snippet_past_the_match() {
        // Only one space, near the start: the "do not cut a word" rule must not fall back to it.
        let filler = "字".repeat(120);
        let mut i = Bm25Index::new();
        i.add_doc("far.md", &format!("# T\n\n字字字字字字 搜索{filler}\n"));
        let s = &i.search("搜索", 1)[0].snippet;
        assert!(s.contains("<mark>搜索</mark>"), "{s}");
        assert!(s.ends_with('…'), "{s}");
    }

    #[test]
    fn a_match_deep_in_a_chinese_paragraph_gets_a_windowed_snippet() {
        let filler = "字".repeat(200);
        let mut i = Bm25Index::new();
        i.add_doc("deep.md", &format!("# T\n\n{filler}搜索{filler}\n"));
        let s = &i.search("搜索", 1)[0].snippet;
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(s.contains("<mark>搜索</mark>"), "{s}");
        assert!(s.chars().count() < 120, "{}", s.chars().count());
    }

    #[test]
    fn chinese_headings_match_and_are_boosted() {
        let mut i = Bm25Index::new();
        i.add_doc("a.md", "# 甲\n\n## 搜索引擎\n\n无关内容。\n");
        i.add_doc(
            "b.md",
            "# 乙\n\n正文里提到一次搜索引擎，还有很多别的字词来拉长这一段文字。\n",
        );
        let hits = i.search("搜索", 5);
        assert_eq!(hits[0].path, "a.md");
        assert!(
            hits[0].snippet.contains("<mark>搜索</mark>"),
            "{}",
            hits[0].snippet
        );
    }
}
