//! Pure word rules: the form a word is looked up in, the repair of dictionary lines the engine cannot read, which
//! suggestions are kept, and which forms a personal word accepts.

use std::borrow::Cow;
use std::collections::BTreeSet;

use permark_core::settings::is_unsafe_char;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::{is_nfc_quick, IsNormalized, UnicodeNormalization};

/// Longest word (chars) judged by the engine or taken into the personal dictionary.
pub const MAX_WORD_CHARS: usize = 100;
/// Longest word (chars) the engine is asked to correct: its time grows with the length, to many seconds past this,
/// while typos of real words stay well below it.
pub const MAX_SUGGEST_CHARS: usize = 32;

/// `word` in the form the dictionaries spell: NFC; typographic apostrophe `’` -> `'` (not every `.aff` maps it); for
/// Romanian (`ro`, `ro-*`) the cedilla letters `ş ţ Ş Ţ` -> the comma letters `ș ț Ș Ț` its dictionary uses. Borrowed
/// when nothing changes.
#[must_use]
pub fn normalize<'a>(word: &'a str, lang: &str) -> Cow<'a, str> {
    let romanian = lang == "ro" || lang.starts_with("ro-");
    let mapped = |c: char| match c {
        '\u{2019}' => '\'',
        'ş' if romanian => 'ș',
        'ţ' if romanian => 'ț',
        'Ş' if romanian => 'Ș',
        'Ţ' if romanian => 'Ț',
        other => other,
    };
    let composed = is_nfc_quick(word.chars()) == IsNormalized::Yes;
    if composed && word.chars().all(|c| mapped(c) == c) {
        return Cow::Borrowed(word);
    }
    // compose first: `s` + U+0327 becomes `ş`, then the comma form
    Cow::Owned(word.nfc().map(mapped).collect())
}

/// How the flags of a dictionary are written (`FLAG` of the `.aff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    /// One byte per flag (default).
    Short,
    /// Two characters per flag; `aliased`: the `.aff` has `AF` aliases (a number names a flag set).
    Long {
        aliased: bool,
    },
    /// Comma-separated numbers 1-65535.
    Numeric,
    Utf8,
}

impl FlagKind {
    /// Flag kind declared by `aff` (none: short).
    #[must_use]
    pub fn of(aff: &str) -> Self {
        let declared = aff.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next() == Some("FLAG"))
                .then(|| parts.next())
                .flatten()
        });
        let aliased = aff.lines().any(|line| line.starts_with("AF "));
        match declared {
            Some("long") => Self::Long { aliased },
            Some("num") => Self::Numeric,
            Some("UTF-8") => Self::Utf8,
            _ => Self::Short,
        }
    }

    /// Whether the engine reads `flags` (the text after a word's `/`).
    fn reads(self, flags: &str) -> bool {
        if flags.is_empty() {
            return true;
        }
        match self {
            Self::Short | Self::Utf8 => !flags.contains('\0'),
            Self::Long { aliased } => {
                flags.len().is_multiple_of(2)
                    || (aliased && flags.bytes().all(|b| b.is_ascii_digit()))
            }
            Self::Numeric => flags
                .split(',')
                .all(|part| part.parse::<u16>().is_ok_and(|n| n != 0)),
        }
    }
}

/// A space followed by a morphological field (`po:`, `st:`): what follows is not part of the word.
fn morph_follows(rest: &str) -> bool {
    let mut chars = rest.trim_start().chars();
    chars.next().is_some_and(char::is_lowercase)
        && chars.next().is_some_and(char::is_lowercase)
        && chars.next() == Some(':')
}

/// Byte range of the flags of a `.dic` line (after its first unescaped `/` past the start, up to whitespace), as the
/// engine splits it. `None`: the line has no flags.
fn flags_of(line: &str) -> Option<(usize, usize)> {
    let mut escaped = false;
    for (at, ch) in line.char_indices() {
        match ch {
            '/' if !escaped && at != 0 => {
                let start = at + 1;
                let end = line[start..]
                    .find(char::is_whitespace)
                    .map_or(line.len(), |n| start + n);
                return Some((start, end));
            }
            '\t' => return None,
            ' ' if morph_follows(&line[at + 1..]) => return None,
            _ => {}
        }
        escaped = ch == '\\';
    }
    None
}

/// `dic` with the flags (and what follows them) dropped from every line whose flags the engine cannot read, the word
/// kept: one bad line would refuse the whole dictionary (e.g. the Galician one has a few dozen). Borrowed when every
/// line reads.
#[must_use]
pub fn sanitize_dic(dic: &str, kind: FlagKind) -> Cow<'_, str> {
    if matches!(kind, FlagKind::Short | FlagKind::Utf8) && !dic.contains('\0') {
        return Cow::Borrowed(dic);
    }
    let mut out: Option<String> = None;
    let mut done = 0;
    // first line = the word count
    let mut at = dic.find('\n').map_or(dic.len(), |n| n + 1);
    while at < dic.len() {
        let end = dic[at..].find('\n').map_or(dic.len(), |n| at + n + 1);
        let line = &dic[at..end];
        let trimmed = line.trim();
        let lead = line.len() - line.trim_start().len();
        if let Some((start, stop)) = flags_of(trimmed).filter(|_| !trimmed.starts_with('/')) {
            if !kind.reads(&trimmed[start..stop]) {
                let text = out.get_or_insert_with(|| String::with_capacity(dic.len()));
                text.push_str(&dic[done..at + lead + start - 1]);
                text.push('\n');
                done = end;
            }
        }
        at = end;
    }
    match out {
        Some(mut text) => {
            text.push_str(&dic[done..]);
            Cow::Owned(text)
        }
        None => Cow::Borrowed(dic),
    }
}

/// Whether a suggestion is offered: one holding a space only when every part is a word `accepts` knows (the engine
/// may split a word into a part that is no word at all, e.g. Portuguese "usa d").
pub fn keep_suggestion(suggestion: &str, accepts: impl Fn(&str) -> bool) -> bool {
    !suggestion.contains(' ')
        || suggestion
            .split(' ')
            .filter(|part| !part.is_empty())
            .all(accepts)
}

/// Whether `word` is longer than [`MAX_WORD_CHARS`]: no dictionary holds such a word, so the engine is never asked
/// about it (it stays unmarked, with no corrections).
#[must_use]
pub fn over_cap(word: &str) -> bool {
    word.chars().nth(MAX_WORD_CHARS).is_some()
}

/// Whether the engine is asked to correct `word`: at most [`MAX_SUGGEST_CHARS`] long and made only of what the words
/// of a text hold (letters, combining marks, apostrophes, middle dot, hyphens). Anything else is no typo of a
/// dictionary word, and correcting it can take the engine many seconds.
#[must_use]
pub fn suggestible(word: &str) -> bool {
    !word.is_empty()
        && word.chars().nth(MAX_SUGGEST_CHARS).is_none()
        && word.chars().all(|c| {
            c.is_alphabetic()
                || is_combining_mark(c)
                || matches!(c, '\'' | '\u{2019}' | '\u{B7}' | '-' | '\u{2010}')
        })
}

/// A word as the personal dictionary keeps it (`None`: not a single word, or one with an invisible character that
/// would hide what it really is).
#[must_use]
pub fn personal_form(word: &str) -> Option<String> {
    let word = normalize(word.trim(), "");
    let fits = !word.is_empty()
        && !over_cap(&word)
        && !word
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || is_unsafe_char(c) || is_format(c));
    fits.then(|| word.into_owned())
}

/// A Unicode format character (category Cf: soft hyphen, zero-width and joiners, bidi marks | embeddings |
/// isolates, word joiner, byte order mark, tags, ...): invisible in a word.
fn is_format(c: char) -> bool {
    matches!(
        c,
        '\u{AD}'
            | '\u{600}'..='\u{605}'
            | '\u{61C}'
            | '\u{6DD}'
            | '\u{70F}'
            | '\u{890}'..='\u{891}'
            | '\u{8E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// Whether the personal `words` accept `word`: as kept, or a lowercase entry in its Capitalized | UPPER form.
#[must_use]
pub fn personal_accepts(words: &BTreeSet<String>, word: &str) -> bool {
    if words.is_empty() {
        return false;
    }
    let word = normalize(word, "");
    if words.contains(word.as_ref()) {
        return true;
    }
    let lower = word.to_lowercase();
    if lower == word || !words.contains(&lower) {
        return false;
    }
    let mut chars = lower.chars();
    let capitalized: String = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    word == capitalized || word == lower.to_uppercase()
}
