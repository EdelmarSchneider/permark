//! Which of the languages with a dictionary a text sample is written in, and the language the person's app starts
//! from. Detection only answers when the sample is long enough and one language clearly leads; otherwise the caller
//! keeps the app's default language (a wrong guess marks every word of a document, a default only some).

use whatlang::{Detector, Lang, Script};

/// Most characters of a sample looked at (the window sends prose of the document, more adds nothing).
pub const MAX_SAMPLE_CHARS: usize = 8192;
/// Fewest letters before a guess is believed (about one sentence).
pub const MIN_LETTERS: usize = 40;
/// Lowest confidence (0..=1, the lead over the next language with a dictionary) a short sample needs: one or two
/// sentences of Catalan read as French | Portuguese below it.
pub const MIN_CONFIDENCE: f64 = 0.7;
/// Letters from which a sample counts as long (about three sentences): its best guess was right even with a narrow
/// lead, so a lower confidence is enough.
pub const LONG_LETTERS: usize = 150;
/// Lowest confidence a long sample needs.
pub const LONG_CONFIDENCE: f64 = 0.3;

/// Languages the detector tells apart, each mapped onto a dictionary by [`id_of`].
const CANDIDATES: [Lang; 13] = [
    Lang::Eng,
    Lang::Por,
    Lang::Spa,
    Lang::Fra,
    Lang::Deu,
    Lang::Ita,
    Lang::Cat,
    Lang::Ron,
    Lang::Pol,
    Lang::Tur,
    Lang::Vie,
    Lang::Rus,
    Lang::Ukr,
];

/// What a sample says about its language.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Detection {
    /// Clearly this language (a dictionary id).
    Found(&'static str),
    /// Too short | no clear lead: the default applies; the best guess when there was one.
    Unsure(Option<&'static str>),
    /// Written in a script none of the dictionaries covers.
    Unsupported(&'static str),
}

/// The language of `sample` among `ids` (dictionary ids); `default` = the app's default language, which decides the
/// region of a language with two dictionaries (`pt-BR` | `pt-PT`, `en` | `en-GB`).
#[must_use]
pub fn detect(sample: &str, ids: &[&'static str], default: &str) -> Detection {
    let sample = match sample.char_indices().nth(MAX_SAMPLE_CHARS) {
        Some((end, _)) => &sample[..end],
        None => sample,
    };
    let Some(script) = whatlang::detect_script(sample) else {
        return Detection::Unsure(None);
    };
    // only languages of the sample's script: the detector answers within that script anyway
    let allowed: Vec<Lang> = CANDIDATES
        .into_iter()
        .filter(|lang| script.langs().contains(lang) && id_of(*lang, ids, default).is_some())
        .collect();
    if allowed.is_empty() {
        return match script {
            Script::Latin => Detection::Unsure(None),
            other => Detection::Unsupported(script_name(other)),
        };
    }
    let letters = sample.chars().filter(|c| c.is_alphabetic()).count();
    if letters < MIN_LETTERS {
        return Detection::Unsure(None);
    }
    let floor = if letters >= LONG_LETTERS {
        LONG_CONFIDENCE
    } else {
        MIN_CONFIDENCE
    };
    let Some(info) = Detector::with_allowlist(allowed).detect(sample) else {
        return Detection::Unsure(None);
    };
    match id_of(info.lang(), ids, default) {
        Some(id) if info.confidence() >= floor => Detection::Found(id),
        guess => Detection::Unsure(guess),
    }
}

/// The dictionary id for a detected language.
#[must_use]
pub fn id_of(lang: Lang, ids: &[&'static str], default: &str) -> Option<&'static str> {
    let has = |id: &str| ids.iter().copied().find(|known| *known == id);
    // the detector has no Galician: its nearest neighbors are Galician for a person whose default is Galician
    if default == "gl" && matches!(lang, Lang::Por | Lang::Spa) {
        if let Some(gl) = has("gl") {
            return Some(gl);
        }
    }
    let (first, second) = match lang {
        Lang::Eng => region_pair(default, "en-GB", "en"),
        Lang::Por => region_pair(default, "pt-PT", "pt-BR"),
        Lang::Spa => ("es", "es"),
        Lang::Fra => ("fr", "fr"),
        Lang::Deu => ("de", "de"),
        Lang::Ita => ("it", "it"),
        Lang::Cat => ("ca", "ca"),
        Lang::Ron => ("ro", "ro"),
        Lang::Pol => ("pl", "pl"),
        Lang::Tur => ("tr", "tr"),
        Lang::Vie => ("vi", "vi"),
        Lang::Rus => ("ru", "ru"),
        Lang::Ukr => ("uk", "uk"),
        _ => return None,
    };
    has(first).or_else(|| has(second))
}

/// `(preferred, other)` of a language with two regional dictionaries: `alternate` only when it is the default.
fn region_pair(
    default: &str,
    alternate: &'static str,
    usual: &'static str,
) -> (&'static str, &'static str) {
    if default == alternate {
        (alternate, usual)
    } else {
        (usual, alternate)
    }
}

/// The default language the app starts from for a window in `ui_locale` (a BCP-47 tag): the same language when it
/// has a dictionary (its first region when only the language matches), else English.
#[must_use]
pub fn locale_default(ui_locale: &str, ids: &[&'static str]) -> &'static str {
    let primary = |tag: &str| {
        tag.split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let wanted = primary(ui_locale);
    ids.iter()
        .find(|id| id.eq_ignore_ascii_case(ui_locale))
        .or_else(|| {
            ids.iter()
                .find(|id| !wanted.is_empty() && primary(id) == wanted)
        })
        .or_else(|| ids.iter().find(|id| **id == "en"))
        .or_else(|| ids.first())
        .copied()
        .unwrap_or("en")
}

/// Stable name of a script for the windows.
fn script_name(script: Script) -> &'static str {
    match script {
        Script::Mandarin => "han",
        Script::Hiragana | Script::Katakana => "kana",
        Script::Hangul => "hangul",
        Script::Cyrillic => "cyrillic",
        Script::Arabic => "arabic",
        Script::Hebrew => "hebrew",
        Script::Greek => "greek",
        Script::Devanagari => "devanagari",
        Script::Thai => "thai",
        _ => "other",
    }
}

#[cfg(test)]
#[path = "detect_tests.rs"]
mod tests;
