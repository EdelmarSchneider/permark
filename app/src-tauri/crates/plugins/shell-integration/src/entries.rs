//! What the file manager entries write, as plain data: keys under `HKEY_CURRENT_USER\Software\Classes`, their values,
//! the command lines and the words. Nothing here touches the registry, so the layout is tested on every platform; the
//! installer (`installer/hooks.nsh`) writes the same layout, and a test keeps the two in step.

use std::path::Path;

use permark_core::desktop::FileGroup;
/// The languages of the entries: the app's (a tag reads as one through `Language::from_tag`, the setting and the
/// system's languages as one through `Language::resolve`).
pub use permark_core::settings::LanguagePref as Language;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One switch of the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ShellEntry")]
pub enum Entry {
    /// "Open with PerMark" on folders, the background of a folder and Markdown files.
    Menu,
    /// "New Markdown file" on folders and the background of a folder.
    NewFile,
}

impl Entry {
    pub const ALL: [Self; 2] = [Self::Menu, Self::NewFile];

    /// The verb (key under `shell`).
    #[must_use]
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Menu => "PerMark",
            Self::NewFile => "PerMarkNew",
        }
    }

    /// The value under `HKCU\Software\PerMark` that remembers the person's choice (1 on, 0 off; shared with the installer).
    #[must_use]
    pub const fn choice(self) -> &'static str {
        match self {
            Self::Menu => "OpenWithMenu",
            Self::NewFile => "NewMarkdownMenu",
        }
    }

    /// Where the entry shows: the class key (under `Software\Classes`) and what the shell puts for the item (`%1` the
    /// clicked item, `%V` the folder whose background was clicked).
    #[must_use]
    pub fn targets(self) -> Vec<(String, &'static str)> {
        let mut targets = vec![
            ("Directory".to_owned(), "%1"),
            (r"Directory\Background".to_owned(), "%V"),
        ];
        if self == Self::Menu {
            targets.extend(
                FileGroup::Markdown
                    .extensions()
                    .iter()
                    .map(|ext| (format!(r"SystemFileAssociations\{ext}"), "%1")),
            );
        }
        targets
    }

    /// The keys of the verb, one per place (relative to `Software\Classes`).
    #[must_use]
    pub fn verb_keys(self) -> Vec<String> {
        self.targets()
            .into_iter()
            .map(|(class, _)| format!(r"{class}\shell\{}", self.verb()))
            .collect()
    }

    /// The command line of a place: the program and the item, each quoted (a path with spaces stays one argument).
    #[must_use]
    pub fn command_line(self, exe: &Path, placeholder: &str) -> String {
        match self {
            Self::Menu => format!("\"{}\" \"{placeholder}\"", exe.display()),
            Self::NewFile => format!("\"{}\" --new-markdown \"{placeholder}\"", exe.display()),
        }
    }

    /// The words of the entry (English for `System`: resolve the language first).
    #[must_use]
    pub fn label(self, language: Language) -> &'static str {
        let (_, open, new_file) = WORDS
            .iter()
            .copied()
            .find(|(of, _, _)| *of == language)
            .unwrap_or(ENGLISH);
        match self {
            Self::Menu => open,
            Self::NewFile => new_file,
        }
    }

    /// Every key and value of the entry for `exe`, worded in `language`.
    #[must_use]
    pub fn keys(self, exe: &Path, language: Language) -> Vec<Key> {
        let icon = format!("\"{}\",0", exe.display());
        self.targets()
            .into_iter()
            .flat_map(|(class, placeholder)| {
                let verb = format!(r"{class}\shell\{}", self.verb());
                [
                    Key {
                        path: verb.clone(),
                        values: vec![
                            (String::new(), self.label(language).to_owned()),
                            ("Icon".to_owned(), icon.clone()),
                        ],
                    },
                    Key {
                        path: format!(r"{verb}\command"),
                        values: vec![(String::new(), self.command_line(exe, placeholder))],
                    },
                ]
            })
            .collect()
    }
}

/// A key (relative to `Software\Classes`) and its string values (`""` = the default value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub path: String,
    pub values: Vec<(String, String)>,
}

/// The words of the entries in English: "Open with PerMark", "New Markdown file with PerMark".
const ENGLISH: (Language, &str, &str) = (
    Language::En,
    "Open with PerMark",
    "New Markdown file with PerMark",
);

/// The words of the entries in each of the app's languages (the installer writes the same, `installer/hooks.nsh`; the
/// Store package's menu too, `crates/shell-ext/src/labels.rs`).
const WORDS: &[(Language, &str, &str)] = &[
    ENGLISH,
    (
        Language::PtBr,
        "Abrir com o PerMark",
        "Novo arquivo Markdown com o PerMark",
    ),
    (
        Language::Es,
        "Abrir con PerMark",
        "Nuevo archivo Markdown con PerMark",
    ),
    (
        Language::De,
        "Mit PerMark öffnen",
        "Neue Markdown-Datei mit PerMark",
    ),
    (
        Language::ZhCn,
        "用 PerMark 打开",
        "用 PerMark 新建 Markdown 文件",
    ),
    (
        Language::It,
        "Apri con PerMark",
        "Nuovo file Markdown con PerMark",
    ),
    (
        Language::Ja,
        "PerMark で開く",
        "PerMark で新しい Markdown ファイルを作成",
    ),
    (
        Language::Ko,
        "PerMark로 열기",
        "PerMark로 새 Markdown 파일 만들기",
    ),
    (
        Language::Fr,
        "Ouvrir avec PerMark",
        "Nouveau fichier Markdown avec PerMark",
    ),
    (
        Language::PtPt,
        "Abrir com o PerMark",
        "Novo ficheiro Markdown com o PerMark",
    ),
    (
        Language::Ca,
        "Obre amb PerMark",
        "Fitxer Markdown nou amb PerMark",
    ),
    (
        Language::Gl,
        "Abrir con PerMark",
        "Novo ficheiro Markdown con PerMark",
    ),
    (
        Language::Ro,
        "Deschide cu PerMark",
        "Fișier Markdown nou cu PerMark",
    ),
    (
        Language::ZhTw,
        "以 PerMark 開啟",
        "以 PerMark 新增 Markdown 檔案",
    ),
    (
        Language::Id,
        "Buka dengan PerMark",
        "File Markdown baru dengan PerMark",
    ),
    (
        Language::Vi,
        "Mở bằng PerMark",
        "Tệp Markdown mới bằng PerMark",
    ),
    (
        Language::Ru,
        "Открыть с помощью PerMark",
        "Новый файл Markdown в PerMark",
    ),
    (
        Language::Tr,
        "PerMark ile aç",
        "PerMark ile yeni Markdown dosyası",
    ),
    (
        Language::Pl,
        "Otwórz w PerMark",
        "Nowy plik Markdown w PerMark",
    ),
    (
        Language::Uk,
        "Відкрити в PerMark",
        "Новий файл Markdown у PerMark",
    ),
];

/// The keys above `key`, nearest first, that are deleted once removing an entry leaves them empty: every parent below
/// the first level of `Software\Classes` (Windows' own `Directory` and `SystemFileAssociations` are never in the list).
#[must_use]
pub fn tidy_parents(key: &str) -> Vec<String> {
    let parts: Vec<&str> = key.split('\\').collect();
    (2..parts.len())
        .rev()
        .map(|n| parts[..n].join("\\"))
        .collect()
}
