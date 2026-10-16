//! What "Open with TrailMark" writes into the Windows registry, described as plain data.
//!
//! Nothing here touches the registry, so the layout, the quoting of the command lines and the wording of the
//! labels are tested on every platform. [`super::registry`] turns these descriptions into registry keys, and the
//! Windows installer (`installer/hooks.nsh`) writes the same layout from NSIS; a test keeps the two in step.
//!
//! Everything lives under `HKEY_CURRENT_USER\Software\Classes`, so no administrator rights are needed. Two groups
//! of entries are independent of each other:
//!
//! * the **context menu**: an "Open with TrailMark" verb on folders, on the background of a folder and on
//!   Markdown files;
//! * the **file association**: a `ProgID` that opens a Markdown file in TrailMark, listed in the "Open with" menu
//!   of `.md` and `.markdown` files. The user's default app is never touched.

// The registry that uses most of this only exists on Windows; the tests run everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;

use crate::session::Language;

/// The name of the verb (the key under `shell`) the context-menu entries use.
pub(crate) const VERB: &str = "TrailMark";

/// The `ProgID` of a Markdown document that TrailMark opens.
pub(crate) const PROG_ID: &str = "TrailMark.Markdown";

/// The extensions the file entry and the association cover.
pub(crate) const EXTENSIONS: [&str; 2] = [".md", ".markdown"];

/// The values, under `HKCU\Software\TrailMark`, that remember what the person chose (1 = on, 0 = off; missing =
/// never chosen). The installer reads them so an update respects the choice, and writes them when it asks.
pub(crate) const MENU_CHOICE: &str = "OpenWithMenu";
pub(crate) const ASSOCIATION_CHOICE: &str = "FileAssociation";

/// A place in the file manager the "Open with TrailMark" verb is added to.
pub(crate) struct MenuTarget {
    /// The class key, under `Software\Classes`, that holds the `shell` verbs.
    pub class: &'static str,
    /// What the shell replaces with the path: `%1` is the item that was clicked, `%V` the folder whose
    /// background was clicked.
    pub placeholder: &'static str,
}

impl MenuTarget {
    /// The key of the verb, relative to `Software\Classes`.
    pub fn key(&self) -> String {
        format!(r"{}\shell\{VERB}", self.class)
    }
}

/// Folders, the background of a folder, and Markdown files (`SystemFileAssociations` applies a verb to an
/// extension whichever program owns it).
pub(crate) const MENU_TARGETS: [MenuTarget; 4] = [
    MenuTarget {
        class: "Directory",
        placeholder: "%1",
    },
    MenuTarget {
        class: r"Directory\Background",
        placeholder: "%V",
    },
    MenuTarget {
        class: r"SystemFileAssociations\.md",
        placeholder: "%1",
    },
    MenuTarget {
        class: r"SystemFileAssociations\.markdown",
        placeholder: "%1",
    },
];

/// A named string value of a key. The empty name is the key's default value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Value {
    pub name: String,
    pub data: String,
}

/// A registry key, relative to `Software\Classes`, and the values to write into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    pub path: String,
    pub values: Vec<Value>,
}

fn text(name: &str, data: impl Into<String>) -> Value {
    Value {
        name: name.to_owned(),
        data: data.into(),
    }
}

fn key(path: impl Into<String>, values: Vec<Value>) -> Key {
    Key {
        path: path.into(),
        values,
    }
}

/// `exe` followed by the placeholder, both quoted so a path with spaces stays one argument each:
/// `"C:\Program Files\TrailMark\trailmark.exe" "%1"`.
pub(crate) fn command_line(exe: &Path, placeholder: &str) -> String {
    format!("\"{}\" \"{placeholder}\"", exe.display())
}

/// The icon of an entry: the first icon of the program.
pub(crate) fn icon_value(exe: &Path) -> String {
    format!("\"{}\",0", exe.display())
}

/// The keys of the context-menu entries: for each place, the verb (label and icon) and its command.
pub(crate) fn menu_keys(exe: &Path, label: &str) -> Vec<Key> {
    MENU_TARGETS
        .iter()
        .flat_map(|target| {
            let verb = target.key();
            [
                key(
                    verb.clone(),
                    vec![text("", label), text("Icon", icon_value(exe))],
                ),
                key(
                    format!(r"{verb}\command"),
                    vec![text("", command_line(exe, target.placeholder))],
                ),
            ]
        })
        .collect()
}

/// The keys of the `ProgID` that opens a Markdown file in TrailMark.
pub(crate) fn association_keys(exe: &Path, type_name: &str) -> Vec<Key> {
    vec![
        key(PROG_ID, vec![text("", type_name)]),
        key(
            format!(r"{PROG_ID}\DefaultIcon"),
            vec![text("", icon_value(exe))],
        ),
        key(
            format!(r"{PROG_ID}\shell\open"),
            vec![text("FriendlyAppName", "TrailMark")],
        ),
        key(
            format!(r"{PROG_ID}\shell\open\command"),
            vec![text("", command_line(exe, "%1"))],
        ),
    ]
}

/// The `OpenWithProgids` key of an extension (relative to `Software\Classes`).
pub(crate) fn open_with_key(extension: &str) -> String {
    format!(r"{extension}\OpenWithProgids")
}

/// An extension's own `shell` key (the place for verbs of that extension). Nothing of TrailMark's is written there, but a
/// leftover empty one is as meaningless as the keys above the entries, and would keep the extension's key from going.
pub(crate) fn extension_shell_key(extension: &str) -> String {
    format!(r"{extension}\shell")
}

/// The keys above `key`, nearest first, that are deleted once turning an entry off leaves nothing in them: every
/// parent below the first level of `Software\Classes`, and the key of an extension (writing the "Open with" list may
/// have created it). The first-level classes of Windows itself (`Directory`, `SystemFileAssociations`) are never in
/// the list, empty or not.
pub(crate) fn tidy_parents(key: &str) -> Vec<&str> {
    let mut parents = Vec::new();
    let mut below = key;
    while let Some((parent, _)) = below.rsplit_once('\\') {
        if !parent.contains('\\') && !EXTENSIONS.contains(&parent) {
            break;
        }
        parents.push(parent);
        below = parent;
    }
    parents
}

/// How the "Open with" list learns about the `ProgID`: a value named after it, with no data, under each extension.
pub(crate) fn open_with_keys() -> Vec<Key> {
    EXTENSIONS
        .iter()
        .map(|extension| key(open_with_key(extension), vec![text(PROG_ID, "")]))
        .collect()
}

/// The text of the context-menu entry, in the language of the interface (English for "System default": the
/// registry cannot follow the system by itself).
pub(crate) fn menu_label(language: Language) -> &'static str {
    match language {
        Language::En | Language::System => "Open with TrailMark",
        Language::PtBr => "Abrir com o TrailMark",
        Language::Es => "Abrir con TrailMark",
        Language::De => "Mit TrailMark öffnen",
        Language::ZhCn => "用 TrailMark 打开",
        Language::It => "Apri con TrailMark",
    }
}

/// What Explorer calls a Markdown file when TrailMark is the app that opens it (the "Type" column).
pub(crate) fn type_name(language: Language) -> &'static str {
    match language {
        Language::En | Language::System => "Markdown document",
        Language::PtBr | Language::It => "Documento Markdown",
        Language::Es => "Documento de Markdown",
        Language::De => "Markdown-Dokument",
        Language::ZhCn => "Markdown 文档",
    }
}

/// The language for a code the UI or the installer uses (`en`, `pt-BR`, `es`, `de`, `zh-CN`, `it`); English for
/// anything else.
pub(crate) fn language_for(code: &str) -> Language {
    Language::from_installer_code(code).unwrap_or(Language::En)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Trail Mark\trailmark.exe")
    }

    fn text_of(keys: &[Key], path: &str, name: &str) -> String {
        let value = keys
            .iter()
            .find(|k| k.path == path)
            .and_then(|k| k.values.iter().find(|v| v.name == name));
        value
            .unwrap_or_else(|| panic!("{path} / {name:?} is not written"))
            .data
            .clone()
    }

    #[test]
    fn the_command_is_the_program_and_one_argument_each_in_quotes() {
        assert_eq!(
            command_line(&exe(), "%1"),
            r#""C:\Program Files\Trail Mark\trailmark.exe" "%1""#
        );
        assert_eq!(
            command_line(
                Path::new(r"C:\Users\Zoë\AppData\Local\TrailMark\trailmark.exe"),
                "%V"
            ),
            r#""C:\Users\Zoë\AppData\Local\TrailMark\trailmark.exe" "%V""#
        );
        assert_eq!(
            icon_value(&exe()),
            r#""C:\Program Files\Trail Mark\trailmark.exe",0"#
        );
    }

    #[test]
    fn the_menu_covers_folders_their_background_and_markdown_files() {
        let keys = menu_keys(&exe(), "Open with TrailMark");
        let paths: Vec<&str> = keys.iter().map(|k| k.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                r"Directory\shell\TrailMark",
                r"Directory\shell\TrailMark\command",
                r"Directory\Background\shell\TrailMark",
                r"Directory\Background\shell\TrailMark\command",
                r"SystemFileAssociations\.md\shell\TrailMark",
                r"SystemFileAssociations\.md\shell\TrailMark\command",
                r"SystemFileAssociations\.markdown\shell\TrailMark",
                r"SystemFileAssociations\.markdown\shell\TrailMark\command",
            ]
        );
    }

    #[test]
    fn a_folder_passes_its_path_and_the_background_passes_the_folder_it_is_in() {
        let keys = menu_keys(&exe(), "x");
        let program = r#""C:\Program Files\Trail Mark\trailmark.exe""#;
        for (path, placeholder) in [
            (r"Directory\shell\TrailMark\command", "%1"),
            (r"Directory\Background\shell\TrailMark\command", "%V"),
            (r"SystemFileAssociations\.md\shell\TrailMark\command", "%1"),
            (
                r"SystemFileAssociations\.markdown\shell\TrailMark\command",
                "%1",
            ),
        ] {
            assert_eq!(
                text_of(&keys, path, ""),
                format!("{program} \"{placeholder}\""),
                "{path}"
            );
        }
    }

    #[test]
    fn each_verb_has_the_label_and_the_icon_of_the_program() {
        let keys = menu_keys(&exe(), "Mit TrailMark öffnen");
        for target in &MENU_TARGETS {
            let verb = target.key();
            assert_eq!(text_of(&keys, &verb, ""), "Mit TrailMark öffnen");
            assert_eq!(
                text_of(&keys, &verb, "Icon"),
                r#""C:\Program Files\Trail Mark\trailmark.exe",0"#
            );
        }
    }

    #[test]
    fn the_association_is_a_prog_id_that_opens_the_file_it_is_given() {
        let keys = association_keys(&exe(), "Markdown document");
        assert_eq!(
            text_of(&keys, "TrailMark.Markdown", ""),
            "Markdown document"
        );
        assert_eq!(
            text_of(&keys, r"TrailMark.Markdown\DefaultIcon", ""),
            r#""C:\Program Files\Trail Mark\trailmark.exe",0"#
        );
        assert_eq!(
            text_of(&keys, r"TrailMark.Markdown\shell\open\command", ""),
            r#""C:\Program Files\Trail Mark\trailmark.exe" "%1""#
        );
    }

    #[test]
    fn the_open_with_list_gets_the_prog_id_for_both_extensions_and_no_default_is_set() {
        let keys = open_with_keys();
        let paths: Vec<&str> = keys.iter().map(|k| k.path.as_str()).collect();
        assert_eq!(
            paths,
            [r".md\OpenWithProgids", r".markdown\OpenWithProgids"]
        );
        for k in &keys {
            // one value named after the ProgID, with no data; nothing that would make TrailMark the default
            assert_eq!(
                k.values,
                [Value {
                    name: "TrailMark.Markdown".to_owned(),
                    data: String::new()
                }]
            );
        }
        let association = association_keys(&exe(), "x");
        assert!(association
            .iter()
            .chain(&menu_keys(&exe(), "x"))
            .all(|k| !k.path.starts_with('.')));
    }

    #[test]
    fn the_keys_above_an_entry_are_tidied_up_but_never_the_first_level_ones_of_windows() {
        assert_eq!(
            tidy_parents(r"Directory\Background\shell\TrailMark"),
            [r"Directory\Background\shell", r"Directory\Background"]
        );
        assert_eq!(
            tidy_parents(r"SystemFileAssociations\.md\shell\TrailMark"),
            [
                r"SystemFileAssociations\.md\shell",
                r"SystemFileAssociations\.md"
            ]
        );
        assert_eq!(
            tidy_parents(r"Directory\shell\TrailMark"),
            [r"Directory\shell"]
        );
        // the key of an extension may go (the "Open with" list can have created it), another first-level key may not
        assert_eq!(tidy_parents(r".md\OpenWithProgids"), [".md"]);
        assert_eq!(
            tidy_parents(&extension_shell_key(".markdown")),
            [".markdown"]
        );
        assert_eq!(tidy_parents("Directory"), Vec::<&str>::new());
        assert_eq!(tidy_parents(r"Other\shell"), Vec::<&str>::new());
        assert_eq!(tidy_parents(PROG_ID), Vec::<&str>::new());
    }

    #[test]
    fn the_labels_are_the_ones_each_language_uses() {
        assert_eq!(menu_label(Language::En), "Open with TrailMark");
        assert_eq!(menu_label(Language::PtBr), "Abrir com o TrailMark");
        assert_eq!(menu_label(Language::Es), "Abrir con TrailMark");
        assert_eq!(menu_label(Language::De), "Mit TrailMark öffnen");
        assert_eq!(menu_label(Language::ZhCn), "用 TrailMark 打开");
        assert_eq!(menu_label(Language::It), "Apri con TrailMark");
        // the registry cannot follow the system's language, so "System default" reads as English
        assert_eq!(menu_label(Language::System), menu_label(Language::En));
        assert_eq!(type_name(Language::En), "Markdown document");
        assert_eq!(type_name(Language::PtBr), "Documento Markdown");
        assert_eq!(type_name(Language::Es), "Documento de Markdown");
        assert_eq!(type_name(Language::De), "Markdown-Dokument");
        assert_eq!(type_name(Language::ZhCn), "Markdown 文档");
        assert_eq!(type_name(Language::It), "Documento Markdown");
    }

    #[test]
    fn language_codes_come_from_the_ui_and_unknown_ones_read_as_english() {
        assert_eq!(language_for("pt-BR"), Language::PtBr);
        assert_eq!(language_for("de"), Language::De);
        assert_eq!(language_for("zh-CN"), Language::ZhCn);
        assert_eq!(language_for("it"), Language::It);
        for code in [
            "", "system", "fr", "pt-br", "zh", "zh-TW", "2052", "1046", "it-IT", "1040",
        ] {
            assert_eq!(language_for(code), Language::En, "{code}");
        }
    }
}
