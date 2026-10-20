//! The words of the two entries, in the app's languages. The menu follows Windows' display language (the rest of the
//! menu is in it); the installer version's registry entries use the same words (a test of the plugin
//! `shell-integration` keeps them in step).

pub const OPEN: [&str; 6] = [
    "Open with PerMark",
    "Abrir com o PerMark",
    "Abrir con PerMark",
    "Mit PerMark öffnen",
    "用 PerMark 打开",
    "Apri con PerMark",
];

pub const NEW_MARKDOWN: [&str; 6] = [
    "New Markdown file with PerMark",
    "Novo arquivo Markdown com o PerMark",
    "Nuevo archivo Markdown con PerMark",
    "Neue Markdown-Datei mit PerMark",
    "用 PerMark 新建 Markdown 文件",
    "Nuovo file Markdown con PerMark",
];

/// Index of a tag's language in the tables (`en`, `pt`, `es`, `de`, `zh`, `it`): only the language counts, so `pt-PT`
/// reads the Brazilian text and every Chinese tag the Simplified one.
fn index(tag: &str) -> Option<usize> {
    let language = tag.split(['-', '_']).next()?.to_ascii_lowercase();
    ["en", "pt", "es", "de", "zh", "it"]
        .iter()
        .position(|l| *l == language)
}

/// The text of `table` for the first of `preferred` (best first) PerMark is translated into; English otherwise.
pub fn pick<S: AsRef<str>>(table: &[&'static str; 6], preferred: &[S]) -> &'static str {
    let found = preferred.iter().find_map(|tag| index(tag.as_ref()));
    table[found.unwrap_or(0)]
}
