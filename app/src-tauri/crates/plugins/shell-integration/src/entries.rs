//! What the file manager entries write, as plain data: keys under `HKEY_CURRENT_USER\Software\Classes`, their values,
//! the command lines and the words. Nothing here touches the registry, so the layout is tested on every platform; the
//! installer (`installer/hooks.nsh`) writes the same layout, and a test keeps the two in step.

use std::path::Path;

use permark_core::desktop::FileGroup;
use permark_core::settings::LanguagePref;
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

    /// The words of the entry.
    #[must_use]
    pub const fn label(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::Menu, Language::En) => "Open with PerMark",
            // pt-BR and pt-PT, es and gl say it the same way
            (Self::Menu, Language::PtBr | Language::PtPt) => "Abrir com o PerMark",
            (Self::Menu, Language::Es | Language::Gl) => "Abrir con PerMark",
            (Self::Menu, Language::De) => "Mit PerMark öffnen",
            (Self::Menu, Language::ZhCn) => "用 PerMark 打开",
            (Self::Menu, Language::It) => "Apri con PerMark",
            (Self::Menu, Language::Ja) => "PerMark で開く",
            (Self::Menu, Language::Ko) => "PerMark로 열기",
            (Self::Menu, Language::Fr) => "Ouvrir avec PerMark",
            (Self::Menu, Language::Ca) => "Obre amb PerMark",
            (Self::Menu, Language::Ro) => "Deschide cu PerMark",
            (Self::NewFile, Language::En) => "New Markdown file with PerMark",
            (Self::NewFile, Language::PtBr) => "Novo arquivo Markdown com o PerMark",
            (Self::NewFile, Language::Es) => "Nuevo archivo Markdown con PerMark",
            (Self::NewFile, Language::De) => "Neue Markdown-Datei mit PerMark",
            (Self::NewFile, Language::ZhCn) => "用 PerMark 新建 Markdown 文件",
            (Self::NewFile, Language::It) => "Nuovo file Markdown con PerMark",
            (Self::NewFile, Language::Ja) => "PerMark で新しい Markdown ファイルを作成",
            (Self::NewFile, Language::Ko) => "PerMark로 새 Markdown 파일 만들기",
            (Self::NewFile, Language::Fr) => "Nouveau fichier Markdown avec PerMark",
            (Self::NewFile, Language::PtPt) => "Novo ficheiro Markdown com o PerMark",
            (Self::NewFile, Language::Ca) => "Fitxer Markdown nou amb PerMark",
            (Self::NewFile, Language::Gl) => "Novo ficheiro Markdown con PerMark",
            (Self::NewFile, Language::Ro) => "Fișier Markdown nou cu PerMark",
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

/// The languages of the entries (the app's thirteen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    PtBr,
    Es,
    De,
    ZhCn,
    It,
    Ja,
    Ko,
    Fr,
    PtPt,
    Ca,
    Gl,
    Ro,
}

impl Language {
    /// From a language tag (`pt-BR`, `de-DE`, `zh-Hans-CN`): only the language counts, but for Portuguese, where Brazil
    /// and a bare `pt` read Brazilian and any other region (`pt-PT`, `pt-AO`) European; `None` for one PerMark lacks.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        let language = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        match language.as_str() {
            "en" => Some(Self::En),
            "pt" if region(tag).is_some_and(|code| code != "br") => Some(Self::PtPt),
            "pt" => Some(Self::PtBr),
            "es" => Some(Self::Es),
            "de" => Some(Self::De),
            "zh" => Some(Self::ZhCn),
            "it" => Some(Self::It),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "fr" => Some(Self::Fr),
            "ca" => Some(Self::Ca),
            "gl" => Some(Self::Gl),
            "ro" => Some(Self::Ro),
            _ => None,
        }
    }

    /// The app's language; "System" = the first of the system's display languages PerMark has, else English.
    #[must_use]
    pub fn resolve<S: AsRef<str>>(pref: LanguagePref, system: &[S]) -> Self {
        match pref {
            LanguagePref::En => Self::En,
            LanguagePref::PtBr => Self::PtBr,
            LanguagePref::Es => Self::Es,
            LanguagePref::De => Self::De,
            LanguagePref::ZhCn => Self::ZhCn,
            LanguagePref::It => Self::It,
            LanguagePref::Ja => Self::Ja,
            LanguagePref::Ko => Self::Ko,
            LanguagePref::Fr => Self::Fr,
            LanguagePref::PtPt => Self::PtPt,
            LanguagePref::Ca => Self::Ca,
            LanguagePref::Gl => Self::Gl,
            LanguagePref::Ro => Self::Ro,
            LanguagePref::System => system
                .iter()
                .find_map(|tag| Self::from_tag(tag.as_ref()))
                .unwrap_or(Self::En),
        }
    }
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
