//! Crafted words through the plugin's own check and suggest path, over each real dictionary: no input may panic the
//! engine (the app is built with `panic = "abort"`, so a panic would end it). The words are drawn from a seeded
//! generator: misspelled and recased dictionary words (case mappings that change a character's byte length: `ß`,
//! `ﬁ`, `ŉ`, `ȿ`, `Ⱥ`), compounds, the `.aff`'s own `REP` patterns at word edges next to multibyte letters, tripled
//! letters, camel | Pascal case, combining marks, right-to-left letters, apostrophes | hyphens | middle dots, random
//! Unicode, and runs up to and past the word cap.
//!
//! Needs the files: `SPELLING_DICTS=<folder with one subfolder per source folder> cargo test -- --ignored`;
//! `SPELLING_FUZZ_WORDS` (default 20 000) words per language, every `SPELLING_FUZZ_SUGGEST`-th (default 10) also asked
//! for corrections, read by `SPELLING_FUZZ_THREADS` (default 2) threads per language; `SPELLING_FUZZ_LANGS` (`ca,gl`)
//! keeps only those languages. Corrections are slow in a debug build: `--config
//! 'profile.dev.package.spellbook.opt-level=3'` helps.

use std::panic::{catch_unwind, AssertUnwindSafe};

use unicode_normalization::UnicodeNormalization;

use super::*;

/// Letters, marks and signs mixed into the words besides the dictionary's own `TRY` letters.
const EXTRA: &[char] = &[
    '\'',
    '\u{2019}',
    '\u{2BC}',
    '\u{2018}',
    '-',
    '\u{2010}',
    '\u{2011}',
    '\u{B7}',
    '.',
    'ß',
    'ẞ',
    'İ',
    'ı',
    'ǅ',
    'ﬁ',
    'ﬀ',
    'ŉ',
    'ǰ',
    'ς',
    'Σ',
    'ΐ',
    'Å',
    '\u{2126}',
    '\u{212A}',
    'ȿ',
    'Ⱥ',
    'ɐ',
    '\u{300}',
    '\u{301}',
    '\u{308}',
    '\u{327}',
    '\u{36F}',
    '\u{200D}',
    '\u{200C}',
    '\u{FE0F}',
    '\u{FFFD}',
    'א',
    'ש',
    '\u{5B4}',
    'ع',
    'ب',
    'ة',
    '日',
    'の',
    '한',
    '😀',
    '\u{1F3FB}',
    '1',
    '9',
    '_',
    ' ',
    '\u{A0}',
    '$',
    '^',
    '/',
    '\\',
    '[',
    ']',
    '*',
    '?',
];
/// Characters whose case mapping changes their length in bytes | characters.
const GROWING: &[char] = &['ß', 'ﬁ', 'ﬀ', 'ŉ', 'ǰ', 'ΐ', 'ȿ', 'Ⱥ', 'ɐ', 'İ', 'ẞ'];
const MARKS: &[char] = &[
    '\u{300}', '\u{301}', '\u{302}', '\u{308}', '\u{327}', '\u{328}', '\u{36F}', '\u{5B4}',
];
const RTL: &[char] = &[
    'א', 'ב', 'ש', 'ת', 'ع', 'ب', 'ة', 'ي', '\u{64B}', '\u{200F}',
];
const JOINERS: &[&str] = &["", "", "-", "\u{2010}", "'", "\u{2019}", "\u{B7}"];

#[test]
#[ignore = "needs the downloaded dictionaries; slow"]
fn crafted_words_never_panic_the_engine() {
    let Ok(root) = std::env::var("SPELLING_DICTS") else {
        return;
    };
    let count = env_number("SPELLING_FUZZ_WORDS", 20_000);
    let every = env_number("SPELLING_FUZZ_SUGGEST", 10).max(1);
    let threads = env_number("SPELLING_FUZZ_THREADS", 2).max(1);
    let only = std::env::var("SPELLING_FUZZ_LANGS").unwrap_or_default();
    let runs: Vec<_> = OFFICIAL
        .into_iter()
        .filter(|language| only.is_empty() || only.split(',').any(|id| id == language.id))
        .map(|language| {
            let root = PathBuf::from(&root);
            std::thread::spawn(move || fuzz(&root, language, count, every, threads))
        })
        .collect();
    let mut failures = Vec::new();
    for run in runs {
        failures.extend(run.join().unwrap());
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn env_number(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// What a run over some words found.
#[derive(Default)]
struct Tally {
    unknown: usize,
    over: usize,
    suggested: usize,
    /// Corrections that took longer than a second: `(time, word)`.
    slow: Vec<(Duration, String)>,
    /// The inputs that panicked (`<lang> check|suggest <word>`).
    failures: Vec<String>,
}

/// The inputs of `language` that panicked; its words are read by `threads` threads.
fn fuzz(
    root: &Path,
    language: Language,
    count: usize,
    every: usize,
    threads: usize,
) -> Vec<String> {
    let (aff, dic, dictionary) = official(root, language);
    let dictionary = Arc::new(dictionary);
    let words = Arc::new(crafted(&aff, &dic, count, &mut Rng::of(language.id)));
    let start = Instant::now();
    let chunk = words.len().div_ceil(threads).max(1);
    let parts: Vec<_> = (0..threads)
        .map(|t| {
            let (dictionary, words) = (Arc::clone(&dictionary), Arc::clone(&words));
            std::thread::spawn(move || {
                let from = (t * chunk).min(words.len());
                let to = (from + chunk).min(words.len());
                run(language.id, &dictionary, &words, from..to, every)
            })
        })
        .collect();
    let mut tally = Tally::default();
    for part in parts {
        let part = part.join().unwrap();
        tally.unknown += part.unknown;
        tally.over += part.over;
        tally.suggested += part.suggested;
        tally.slow.extend(part.slow);
        tally.failures.extend(part.failures);
    }
    tally.slow.sort_by_key(|(took, _)| std::cmp::Reverse(*took));
    let slowest: Vec<String> = tally
        .slow
        .iter()
        .take(3)
        .map(|(time, word)| format!("{time:.1?} {word:?}"))
        .collect();
    eprintln!(
        "{}: {} words checked ({} unknown, {} over the cap), {} asked for corrections ({} over 1 s, slowest {slowest:?}), {} panics, {:.1?}",
        language.id,
        words.len(),
        tally.unknown,
        tally.over,
        tally.suggested,
        tally.slow.len(),
        tally.failures.len(),
        start.elapsed()
    );
    tally.failures
}

/// Checks `words[range]` through the plugin's path; every `every`-th word (by index) is asked for corrections too.
fn run(
    lang: &'static str,
    dictionary: &Arc<Dictionary>,
    words: &[String],
    range: std::ops::Range<usize>,
    every: usize,
) -> Tally {
    let dictionaries = [(lang, Arc::clone(dictionary))];
    let personal = BTreeSet::new();
    let mut tally = Tally::default();
    for n in range {
        let word = &words[n];
        tally.over += usize::from(over_cap(word));
        match catch_unwind(AssertUnwindSafe(|| {
            accepted(&dictionaries, &personal, word)
        })) {
            Ok(known) => tally.unknown += usize::from(!known),
            Err(_) => tally.failures.push(format!("{lang} check {word:?}")),
        }
        if n % every == 0 {
            tally.suggested += 1;
            let start = Instant::now();
            let asked = catch_unwind(AssertUnwindSafe(|| suggestions_in(dictionary, lang, word)));
            let took = start.elapsed();
            if took > Duration::from_secs(1) {
                tally.slow.push((took, word.clone()));
            }
            if asked.is_err() {
                tally.failures.push(format!("{lang} suggest {word:?}"));
            }
        }
    }
    tally
}

/// A small seeded generator (xorshift64*): the same words on every run.
struct Rng(u64);

impl Rng {
    fn of(seed: &str) -> Self {
        let mixed = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        Self(mixed | 1)
    }

    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number below `n` (`0` when `n` is 0).
    fn below(&mut self, n: usize) -> usize {
        let Ok(n) = u64::try_from(n) else { return 0 };
        if n == 0 {
            return 0;
        }
        usize::try_from(self.step() % n).unwrap_or(0)
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// What the generator draws from: the dictionary's words, letters and `REP` patterns.
struct Parts<'a> {
    stems: Vec<&'a str>,
    letters: Vec<char>,
    reps: Vec<(String, String)>,
}

fn crafted(aff: &str, dic: &str, count: usize, rng: &mut Rng) -> Vec<String> {
    let stems: Vec<&str> = dic.lines().skip(1).filter_map(stem_of).collect();
    let mut letters: Vec<char> = Vec::new();
    let mut reps = Vec::new();
    for line in aff.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["TRY" | "WORDCHARS", chars, ..] => letters.extend(chars.chars()),
            ["MAP", chars] if chars.parse::<u32>().is_err() => {
                letters.extend(chars.chars().filter(|c| !matches!(c, '(' | ')')));
            }
            ["REP" | "ICONV" | "OCONV", from, to, ..] => reps.push((
                from.trim_matches(['^', '$']).replace('_', " "),
                to.trim_matches(['^', '$']).replace('_', " "),
            )),
            _ => {}
        }
    }
    letters.extend(EXTRA);
    let parts = Parts {
        stems,
        letters,
        reps,
    };
    (0..count).map(|_| word(rng, &parts)).collect()
}

/// The word of a `.dic` line (flags after the first `/` past the start, fields after whitespace, dropped).
fn stem_of(line: &str) -> Option<&str> {
    let word = line.split_whitespace().next()?;
    let word = word
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '/')
        .map_or(word, |(at, _)| &word[..at]);
    (!word.is_empty()).then_some(word)
}

fn stem(rng: &mut Rng, parts: &Parts<'_>) -> String {
    if parts.stems.is_empty() {
        return "a".to_owned();
    }
    rng.pick(&parts.stems).to_owned()
}

fn word(rng: &mut Rng, parts: &Parts<'_>) -> String {
    match rng.below(13) {
        0 | 1 => {
            let base = stem(rng, parts);
            mutate(rng, &base, &parts.letters)
        }
        2 => {
            let base = stem(rng, parts);
            let base = mutate(rng, &base, &parts.letters);
            recase(rng, &base)
        }
        3 => {
            let n = 2 + rng.below(3);
            let mut out = String::new();
            for i in 0..n {
                if i > 0 {
                    out.push_str(rng.pick(JOINERS));
                }
                let piece = stem(rng, parts);
                out.push_str(&if rng.chance(30) {
                    recase(rng, &piece)
                } else {
                    piece
                });
            }
            out
        }
        4 => rep_edge(rng, parts),
        5 => (0..=rng.below(40))
            .map(|_| rng.pick(&parts.letters))
            .collect(),
        6 => long_run(rng, parts),
        7 => {
            let base = stem(rng, parts);
            if rng.chance(50) {
                base.nfd().collect()
            } else {
                base.chars()
                    .flat_map(|c| {
                        let mark = rng.chance(30).then(|| rng.pick(MARKS));
                        std::iter::once(c).chain(mark)
                    })
                    .collect()
            }
        }
        8 => {
            let base = stem(rng, parts);
            let at = rng.below(base.chars().count() + 1);
            let rtl: String = (0..=rng.below(6)).map(|_| rng.pick(RTL)).collect();
            insert_at(&base, at, &rtl)
        }
        9 => {
            let base = stem(rng, parts);
            let sign = rng.pick(&["'", "\u{2019}", "-", "\u{B7}", "''", "--", "\u{B7}\u{B7}"]);
            match rng.below(4) {
                0 => format!("{sign}{base}"),
                1 => format!("{base}{sign}"),
                2 => sign.repeat(1 + rng.below(4)),
                _ => insert_at(&base, 1, sign),
            }
        }
        10 => {
            let base = stem(rng, parts);
            let chars: Vec<char> = base.chars().collect();
            let at = rng.below(chars.len());
            let c = chars.get(at).copied().unwrap_or('a');
            insert_at(&base, at, &c.to_string().repeat(2 + rng.below(4)))
        }
        11 => {
            let base = stem(rng, parts);
            let mut out = String::new();
            for c in base.chars() {
                if rng.chance(15) {
                    out.push(rng.pick(GROWING));
                }
                out.push(c);
            }
            recase(rng, &out)
        }
        _ => {
            let base = stem(rng, parts);
            recase(rng, &base)
        }
    }
}

/// `base` with one to four random edits: insert, delete, swap, double, replace.
fn mutate(rng: &mut Rng, base: &str, letters: &[char]) -> String {
    let mut chars: Vec<char> = base.chars().collect();
    for _ in 0..=rng.below(4) {
        let at = rng.below(chars.len() + 1);
        match rng.below(5) {
            0 => chars.insert(at, rng.pick(letters)),
            1 if at < chars.len() => {
                chars.remove(at);
            }
            2 if at + 1 < chars.len() => chars.swap(at, at + 1),
            3 if at < chars.len() => chars.insert(at, chars[at]),
            _ if at < chars.len() => chars[at] = rng.pick(letters),
            _ => chars.push(rng.pick(letters)),
        }
    }
    chars.into_iter().collect()
}

/// UPPER, lower, Capitalized, or camel | Pascal case (each letter upper at random, often the first and last).
fn recase(rng: &mut Rng, base: &str) -> String {
    match rng.below(4) {
        0 => base.to_uppercase(),
        1 => base.to_lowercase(),
        2 => {
            let mut chars = base.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
        _ => {
            let last = base.chars().count().saturating_sub(1);
            base.chars()
                .enumerate()
                .flat_map(|(i, c)| {
                    let upper = ((i == 0 || i == last) && rng.chance(70)) || rng.chance(25);
                    if upper {
                        c.to_uppercase().collect::<Vec<char>>()
                    } else {
                        vec![c]
                    }
                })
                .collect()
        }
    }
}

/// A `REP` pattern of the `.aff` at a word's start | end | in its middle, next to a multibyte letter at times.
fn rep_edge(rng: &mut Rng, parts: &Parts<'_>) -> String {
    if parts.reps.is_empty() {
        let base = stem(rng, parts);
        return mutate(rng, &base, &parts.letters);
    }
    let (from, to) = parts.reps[rng.below(parts.reps.len())].clone();
    let piece = if rng.chance(70) { from } else { to };
    let base = stem(rng, parts);
    let multibyte = if rng.chance(40) {
        rng.pick(&['ä', 'é', 'ß', 'ș', 'ç', 'ñ', 'ȿ', '日'])
            .to_string()
    } else {
        String::new()
    };
    let out = match rng.below(4) {
        0 => format!("{base}{multibyte}{piece}"),
        1 => format!("{piece}{multibyte}{base}"),
        2 => {
            let at = rng.below(base.chars().count() + 1);
            insert_at(&base, at, &format!("{multibyte}{piece}"))
        }
        _ => format!("{multibyte}{piece}"),
    };
    if rng.chance(30) {
        recase(rng, &out)
    } else {
        out
    }
}

/// A run of one letter | word up to the cap, now and then past it.
fn long_run(rng: &mut Rng, parts: &Parts<'_>) -> String {
    let length = if rng.chance(10) {
        MAX_WORD_CHARS + 1 + rng.below(300)
    } else {
        40 + rng.below(MAX_WORD_CHARS - 39)
    };
    let unit: String = if rng.chance(50) {
        rng.pick(&parts.letters).to_string()
    } else {
        stem(rng, parts)
    };
    unit.chars().cycle().take(length).collect()
}

/// `base` with `piece` inserted before its `at`-th character.
fn insert_at(base: &str, at: usize, piece: &str) -> String {
    let byte = base.char_indices().nth(at).map_or(base.len(), |(b, _)| b);
    format!("{}{piece}{}", &base[..byte], &base[byte..])
}
