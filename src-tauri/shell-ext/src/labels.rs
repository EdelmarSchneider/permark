//! The words of the menu entry, in the languages of the app.
//!
//! They are the texts of the registry entry of the installer version (`entries::menu_label` in the program), and a test
//! there keeps the two in step. The menu follows the display language of Windows, because that is what the rest of the
//! menu is in; the registry entry follows the language picked in the app.

pub(crate) const ENGLISH: &str = "Open with TrailMark";
pub(crate) const PORTUGUESE: &str = "Abrir com o TrailMark";
pub(crate) const SPANISH: &str = "Abrir con TrailMark";
pub(crate) const GERMAN: &str = "Mit TrailMark öffnen";
pub(crate) const CHINESE: &str = "用 TrailMark 打开";

/// The text for the first of `preferred` (Windows' display languages, best first, as tags such as `pt-BR`) that the app
/// is translated into. English when none is.
pub(crate) fn for_languages<S: AsRef<str>>(preferred: &[S]) -> &'static str {
    preferred
        .iter()
        .find_map(|tag| for_language(tag.as_ref()))
        .unwrap_or(ENGLISH)
}

/// The text for one language tag. Only the language counts, not the region: `pt-PT` reads the Brazilian text, which is
/// the one translation of Portuguese the app has, and every Chinese tag (`zh-Hans-CN`, `zh-TW`) reads the Simplified text.
fn for_language(tag: &str) -> Option<&'static str> {
    let language = tag.split(['-', '_']).next()?;
    match language.to_ascii_lowercase().as_str() {
        "en" => Some(ENGLISH),
        "pt" => Some(PORTUGUESE),
        "es" => Some(SPANISH),
        "de" => Some(GERMAN),
        "zh" => Some(CHINESE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_the_app_has_gets_its_own_text() {
        assert_eq!(for_languages(&["en-US"]), "Open with TrailMark");
        assert_eq!(for_languages(&["pt-BR"]), "Abrir com o TrailMark");
        assert_eq!(for_languages(&["es-ES"]), "Abrir con TrailMark");
        assert_eq!(for_languages(&["de-DE"]), "Mit TrailMark öffnen");
        assert_eq!(for_languages(&["zh-CN"]), "用 TrailMark 打开");
    }

    #[test]
    fn the_region_does_not_matter() {
        assert_eq!(for_languages(&["pt-PT"]), PORTUGUESE);
        assert_eq!(for_languages(&["es-MX"]), SPANISH);
        assert_eq!(for_languages(&["de-AT"]), GERMAN);
        assert_eq!(for_languages(&["en-GB"]), ENGLISH);
        assert_eq!(for_languages(&["PT-br"]), PORTUGUESE);
        assert_eq!(for_languages(&["de"]), GERMAN);
        assert_eq!(for_languages(&["de_DE"]), GERMAN);
    }

    #[test]
    fn every_chinese_tag_reads_the_simplified_text() {
        for tag in [
            "zh",
            "zh-CN",
            "zh-Hans-CN",
            "zh_CN",
            "ZH-cn",
            "zh-SG",
            "zh-TW",
            "zh-Hant-HK",
        ] {
            assert_eq!(for_languages(&[tag]), CHINESE, "{tag}");
        }
        assert_eq!(for_languages(&["fr-FR", "zh-Hans-CN", "en-US"]), CHINESE);
    }

    #[test]
    fn the_first_language_the_app_has_wins() {
        // a French first choice is skipped; the next one the app is in is used
        assert_eq!(for_languages(&["fr-FR", "de-DE", "en-US"]), GERMAN);
        assert_eq!(for_languages(&["es-ES", "de-DE"]), SPANISH);
    }

    #[test]
    fn other_languages_and_an_empty_list_read_as_english() {
        assert_eq!(for_languages(&["fr-FR", "ja-JP"]), ENGLISH);
        assert_eq!(for_languages::<&str>(&[]), ENGLISH);
        assert_eq!(for_languages(&[""]), ENGLISH);
        // a tag that only starts like a supported one
        assert_eq!(for_languages(&["deu-DE", "pty", "zho-CN"]), ENGLISH);
    }
}
