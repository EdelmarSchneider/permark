//! The words of the two entries, in the app's languages. The menu follows Windows' display language (the rest of the
//! menu is in it); the installer version's registry entries use the same words (a test of the plugin
//! `shell-integration` keeps them in step).

/// The words of one language: its code (as `code` reads a tag), "Open with PerMark", "New Markdown file with PerMark".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    code: &'static str,
    pub open: &'static str,
    pub new_markdown: &'static str,
}

const ENGLISH: Words = Words {
    code: "en",
    open: "Open with PerMark",
    new_markdown: "New Markdown file with PerMark",
};

/// The app's languages; `pt-pt` stands for every Portuguese outside Brazil, `zh-tw` for Traditional Chinese.
pub const WORDS: &[Words] = &[
    ENGLISH,
    Words {
        code: "pt",
        open: "Abrir com o PerMark",
        new_markdown: "Novo arquivo Markdown com o PerMark",
    },
    Words {
        code: "es",
        open: "Abrir con PerMark",
        new_markdown: "Nuevo archivo Markdown con PerMark",
    },
    Words {
        code: "de",
        open: "Mit PerMark öffnen",
        new_markdown: "Neue Markdown-Datei mit PerMark",
    },
    Words {
        code: "zh",
        open: "用 PerMark 打开",
        new_markdown: "用 PerMark 新建 Markdown 文件",
    },
    Words {
        code: "it",
        open: "Apri con PerMark",
        new_markdown: "Nuovo file Markdown con PerMark",
    },
    Words {
        code: "ja",
        open: "PerMark で開く",
        new_markdown: "PerMark で新しい Markdown ファイルを作成",
    },
    Words {
        code: "ko",
        open: "PerMark로 열기",
        new_markdown: "PerMark로 새 Markdown 파일 만들기",
    },
    Words {
        code: "fr",
        open: "Ouvrir avec PerMark",
        new_markdown: "Nouveau fichier Markdown avec PerMark",
    },
    Words {
        code: "pt-pt",
        open: "Abrir com o PerMark",
        new_markdown: "Novo ficheiro Markdown com o PerMark",
    },
    Words {
        code: "ca",
        open: "Obre amb PerMark",
        new_markdown: "Fitxer Markdown nou amb PerMark",
    },
    Words {
        code: "gl",
        open: "Abrir con PerMark",
        new_markdown: "Novo ficheiro Markdown con PerMark",
    },
    Words {
        code: "ro",
        open: "Deschide cu PerMark",
        new_markdown: "Fișier Markdown nou cu PerMark",
    },
    Words {
        code: "zh-tw",
        open: "以 PerMark 開啟",
        new_markdown: "以 PerMark 新增 Markdown 檔案",
    },
    Words {
        code: "id",
        open: "Buka dengan PerMark",
        new_markdown: "File Markdown baru dengan PerMark",
    },
    Words {
        code: "vi",
        open: "Mở bằng PerMark",
        new_markdown: "Tệp Markdown mới bằng PerMark",
    },
    Words {
        code: "ru",
        open: "Открыть с помощью PerMark",
        new_markdown: "Новый файл Markdown в PerMark",
    },
    Words {
        code: "tr",
        open: "PerMark ile aç",
        new_markdown: "PerMark ile yeni Markdown dosyası",
    },
    Words {
        code: "pl",
        open: "Otwórz w PerMark",
        new_markdown: "Nowy plik Markdown w PerMark",
    },
    Words {
        code: "uk",
        open: "Відкрити в PerMark",
        new_markdown: "Новий файл Markdown у PerMark",
    },
];

/// The code of a tag's language in the table: only the language counts, but Portuguese reads the region (Brazil and a
/// bare `pt` the Brazilian text, any other region the European one) and Chinese the script, else the region
/// (`zh-Hant`, Taiwan, Hong Kong and Macao the Traditional text, any other the Simplified one).
fn code(tag: &str) -> Option<String> {
    let language = tag.split(['-', '_']).next()?.to_ascii_lowercase();
    let code = match language.as_str() {
        "pt" if region(tag).is_some_and(|code| code != "br") => "pt-pt".to_owned(),
        "zh" if traditional(tag) => "zh-tw".to_owned(),
        _ => language,
    };
    Some(code)
}

fn traditional(tag: &str) -> bool {
    match script(tag) {
        Some(script) => script == "hant",
        None => region(tag).is_some_and(|code| ["tw", "hk", "mo"].contains(&code.as_str())),
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

/// The words for the first of `preferred` (best first) PerMark is translated into; English otherwise.
pub fn pick<S: AsRef<str>>(preferred: &[S]) -> &'static Words {
    preferred
        .iter()
        .filter_map(|tag| code(tag.as_ref()))
        .find_map(|code| WORDS.iter().find(|words| words.code == code))
        .unwrap_or(&ENGLISH)
}
