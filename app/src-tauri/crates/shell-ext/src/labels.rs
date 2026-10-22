//! The words of the two entries, in the app's languages. The menu follows Windows' display language (the rest of the
//! menu is in it); the installer version's registry entries use the same words (a test of the plugin
//! `shell-integration` keeps them in step).

pub const OPEN: [&str; 13] = [
    "Open with PerMark",
    "Abrir com o PerMark",
    "Abrir con PerMark",
    "Mit PerMark öffnen",
    "用 PerMark 打开",
    "Apri con PerMark",
    "PerMark で開く",
    "PerMark로 열기",
    "Ouvrir avec PerMark",
    "Abrir com o PerMark",
    "Obre amb PerMark",
    "Abrir con PerMark",
    "Deschide cu PerMark",
];

pub const NEW_MARKDOWN: [&str; 13] = [
    "New Markdown file with PerMark",
    "Novo arquivo Markdown com o PerMark",
    "Nuevo archivo Markdown con PerMark",
    "Neue Markdown-Datei mit PerMark",
    "用 PerMark 新建 Markdown 文件",
    "Nuovo file Markdown con PerMark",
    "PerMark で新しい Markdown ファイルを作成",
    "PerMark로 새 Markdown 파일 만들기",
    "Nouveau fichier Markdown avec PerMark",
    "Novo ficheiro Markdown com o PerMark",
    "Fitxer Markdown nou amb PerMark",
    "Novo ficheiro Markdown con PerMark",
    "Fișier Markdown nou cu PerMark",
];

/// The tables' languages, in order; `pt-pt` stands for every Portuguese outside Brazil.
const LANGUAGES: [&str; 13] = [
    "en", "pt", "es", "de", "zh", "it", "ja", "ko", "fr", "pt-pt", "ca", "gl", "ro",
];

/// Index of a tag's language in the tables: only the language counts, so every Chinese tag reads the Simplified text,
/// but for Portuguese, where Brazil and a bare `pt` read the Brazilian text and any other region (`pt-PT`, `pt-AO`) the
/// European one.
fn index(tag: &str) -> Option<usize> {
    let language = tag.split(['-', '_']).next()?.to_ascii_lowercase();
    let european = language == "pt" && region(tag).is_some_and(|code| code != "br");
    let code = if european { "pt-pt" } else { language.as_str() };
    LANGUAGES.iter().position(|l| *l == code)
}

/// The region of a tag, after the language and a script if any (`pt-Latn-PT` -> `pt`), lowercase; `None` without one.
fn region(tag: &str) -> Option<String> {
    let mut subtags = tag.split(['-', '_']).skip(1);
    let mut subtag = subtags.next()?;
    if subtag.len() == 4 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
        subtag = subtags.next()?;
    }
    let letters = subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic());
    let digits = subtag.len() == 3 && subtag.bytes().all(|b| b.is_ascii_digit());
    (letters || digits).then(|| subtag.to_ascii_lowercase())
}

/// The text of `table` for the first of `preferred` (best first) PerMark is translated into; English otherwise.
pub fn pick<S: AsRef<str>>(table: &[&'static str; 13], preferred: &[S]) -> &'static str {
    let found = preferred.iter().find_map(|tag| index(tag.as_ref()));
    table[found.unwrap_or(0)]
}
