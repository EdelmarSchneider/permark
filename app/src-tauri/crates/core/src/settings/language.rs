//! Which app language a language tag (BCP 47 | OS: `pt-BR`, `zh_Hant_TW`) stands for; mirror of `matchLocale` in
//! `app/src/core/i18n/locales.ts`.

use super::LanguagePref;

/// Chinese regions that write Traditional characters.
const TRADITIONAL_REGIONS: [&str; 3] = ["tw", "hk", "mo"];

impl LanguagePref {
    /// The app language of `tag`: only the language counts, but Portuguese reads the region (Brazil and a bare `pt`
    /// Brazilian, any other region European) and Chinese the script, else the region (`zh-Hant`, Taiwan, Hong Kong and
    /// Macao Traditional, any other Simplified). `None` for a language PerMark lacks; never `System`.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        let language = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        match language.as_str() {
            "en" => Some(Self::En),
            "pt" if region(tag).is_some_and(|code| code != "br") => Some(Self::PtPt),
            "pt" => Some(Self::PtBr),
            "es" => Some(Self::Es),
            "de" => Some(Self::De),
            "zh" if traditional(tag) => Some(Self::ZhTw),
            "zh" => Some(Self::ZhCn),
            "it" => Some(Self::It),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "fr" => Some(Self::Fr),
            "ca" => Some(Self::Ca),
            "gl" => Some(Self::Gl),
            "ro" => Some(Self::Ro),
            "id" => Some(Self::Id),
            "vi" => Some(Self::Vi),
            "ru" => Some(Self::Ru),
            "tr" => Some(Self::Tr),
            _ => None,
        }
    }

    /// The language the app shows: the person's choice, or for `System` the first of `system` (best first) PerMark
    /// has, else English. Never `System`.
    #[must_use]
    pub fn resolve<S: AsRef<str>>(self, system: &[S]) -> Self {
        match self {
            Self::System => system
                .iter()
                .find_map(|tag| Self::from_tag(tag.as_ref()))
                .unwrap_or(Self::En),
            chosen => chosen,
        }
    }
}

/// Chinese written in Traditional characters: the script says so, or without a script the region does.
fn traditional(tag: &str) -> bool {
    match script(tag) {
        Some(script) => script == "hant",
        None => region(tag).is_some_and(|code| TRADITIONAL_REGIONS.contains(&code.as_str())),
    }
}

/// The script right after the language (`zh-Hant-TW` -> `hant`), lowercase; `None` without one.
fn script(tag: &str) -> Option<String> {
    let subtag = tag.split(['-', '_']).nth(1)?;
    (subtag.len() == 4 && subtag.bytes().all(|b| b.is_ascii_alphabetic()))
        .then(|| subtag.to_ascii_lowercase())
}

/// The region of a tag, after the language and a script if any (`pt-Latn-PT` -> `pt`), lowercase; `None` without one.
fn region(tag: &str) -> Option<String> {
    let subtag = tag
        .split(['-', '_'])
        .nth(if script(tag).is_some() { 2 } else { 1 })?;
    let letters = subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic());
    let digits = subtag.len() == 3 && subtag.bytes().all(|b| b.is_ascii_digit());
    (letters || digits).then(|| subtag.to_ascii_lowercase())
}
