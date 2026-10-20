use std::path::Path;

use permark_core::desktop::FileGroup;
use permark_core::settings::LanguagePref;

use crate::entries::{tidy_parents, Entry, Language};

/// The installer writes the same entries from NSIS.
const HOOKS: &str = include_str!("../../../../installer/hooks.nsh");

#[test]
fn the_menu_shows_on_folders_their_background_and_every_markdown_extension() {
    let keys = Entry::Menu.verb_keys();
    assert_eq!(keys[0], r"Directory\shell\PerMark");
    assert_eq!(keys[1], r"Directory\Background\shell\PerMark");
    for ext in FileGroup::Markdown.extensions() {
        assert!(
            keys.contains(&format!(r"SystemFileAssociations\{ext}\shell\PerMark")),
            "{ext}"
        );
    }
    assert_eq!(
        Entry::NewFile.verb_keys(),
        [
            r"Directory\shell\PerMarkNew",
            r"Directory\Background\shell\PerMarkNew"
        ]
    );
}

#[test]
fn command_lines_quote_the_program_and_the_item() {
    let exe = Path::new(r"C:\Users\Zoë Ann\AppData\Local\PerMark\permark.exe");
    assert_eq!(
        Entry::Menu.command_line(exe, "%1"),
        r#""C:\Users\Zoë Ann\AppData\Local\PerMark\permark.exe" "%1""#
    );
    assert_eq!(
        Entry::NewFile.command_line(exe, "%V"),
        r#""C:\Users\Zoë Ann\AppData\Local\PerMark\permark.exe" --new-markdown "%V""#
    );
    let keys = Entry::Menu.keys(exe, Language::De);
    // verb + command per place
    assert_eq!(keys.len(), Entry::Menu.targets().len() * 2);
    let background = keys
        .iter()
        .find(|k| k.path == r"Directory\Background\shell\PerMark\command")
        .unwrap();
    assert!(background.values[0].1.ends_with(r#" "%V""#));
    let verb = keys
        .iter()
        .find(|k| k.path == r"Directory\shell\PerMark")
        .unwrap();
    assert_eq!(
        verb.values[0],
        (String::new(), "Mit PerMark öffnen".to_owned())
    );
    assert_eq!(
        verb.values[1],
        ("Icon".to_owned(), format!("\"{}\",0", exe.display()))
    );
}

#[test]
fn languages_resolve_from_the_setting_or_the_system() {
    assert_eq!(
        Language::resolve(LanguagePref::De, &["pt-BR"]),
        Language::De
    );
    assert_eq!(
        Language::resolve(LanguagePref::System, &["fr-FR", "pt-PT"]),
        Language::PtBr
    );
    assert_eq!(
        Language::resolve(LanguagePref::System, &["zh-Hant-TW"]),
        Language::ZhCn
    );
    assert_eq!(
        Language::resolve(LanguagePref::System, &["fr-FR"]),
        Language::En
    );
    assert_eq!(
        Language::resolve::<&str>(LanguagePref::System, &[]),
        Language::En
    );
    assert_eq!(Language::from_tag(""), None);
    assert_eq!(Language::from_tag("IT_it"), Some(Language::It));
}

#[test]
fn only_parents_below_windows_own_classes_are_tidied() {
    assert_eq!(
        tidy_parents(r"Directory\shell\PerMark"),
        [r"Directory\shell"]
    );
    assert_eq!(
        tidy_parents(r"Directory\Background\shell\PerMark"),
        [r"Directory\Background\shell", r"Directory\Background"]
    );
    assert_eq!(
        tidy_parents(r"SystemFileAssociations\.md\shell\PerMark"),
        [
            r"SystemFileAssociations\.md\shell",
            r"SystemFileAssociations\.md"
        ]
    );
    assert_eq!(tidy_parents("Directory"), Vec::<String>::new());
}

#[test]
fn the_installer_writes_the_same_entries_and_words() {
    for entry in Entry::ALL {
        for key in entry.verb_keys() {
            assert!(
                HOOKS.contains(&format!("\"{key}\"")),
                "hooks.nsh lacks {key}"
            );
        }
        assert!(
            HOOKS.contains(&format!("\"{}\"", entry.choice())),
            "{}",
            entry.choice()
        );
        for (language, lcid) in [
            (Language::En, 1033),
            (Language::PtBr, 1046),
            (Language::Es, 3082),
            (Language::De, 1031),
            (Language::ZhCn, 2052),
            (Language::It, 1040),
        ] {
            let line = format!(" {lcid} \"{}\"", entry.label(language));
            assert!(HOOKS.contains(&line), "hooks.nsh lacks {line}");
        }
    }
    for group in FileGroup::ALL {
        assert!(HOOKS.contains(group.prog_id()), "{}", group.prog_id());
        for ext in group.extensions() {
            assert!(HOOKS.contains(&format!("\"{ext}\"")), "{ext}");
        }
    }
    // the installer never takes over: no default value of an extension key, no UserChoice
    assert!(!HOOKS.contains("UserChoice"));
    assert!(!HOOKS.contains("APP_ASSOCIATE"));
}

#[cfg(windows)]
mod windows {
    use std::path::Path;

    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    use crate::entries::{Entry, Language};
    use crate::registry::Registry;
    use crate::ShellState;

    /// A throwaway root under `HKCU\Software\PerMarkTest`, deleted when the test ends.
    struct Scratch {
        root: String,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self {
                root: format!(
                    r"Software\PerMarkTest\{name}-{}-{nanos}",
                    std::process::id()
                ),
            }
        }
        fn registry(&self) -> Registry {
            Registry::under(&self.root)
        }
        fn class(&self, key: &str) -> Option<RegKey> {
            RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey(format!(r"{}\Classes\{key}", self.root))
                .ok()
        }
        fn make(&self, key: &str, value: Option<(&str, &str)>) {
            let (k, _) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(format!(r"{}\Classes\{key}", self.root))
                .unwrap();
            if let Some((name, data)) = value {
                k.set_value(name, &data).unwrap();
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let _ = hkcu.delete_subkey_all(&self.root);
            // the shared parent goes when the last test has left it empty
            let _ = hkcu.delete_subkey(r"Software\PerMarkTest");
        }
    }

    /// Windows' own first-level keys stay (here: made by the test's writes), everything below them goes.
    fn assert_nothing_under_directory(s: &Scratch) {
        for key in [r"Directory\shell", r"Directory\Background"] {
            assert!(s.class(key).is_none(), "{key} left behind");
        }
    }

    const EXE: &str = r"C:\Program Files\PerMark ü\permark.exe";

    #[test]
    fn an_empty_registry_has_nothing() {
        let s = Scratch::new("empty");
        assert_eq!(s.registry().state(), ShellState::default());
        assert_eq!(s.registry().choice(Entry::Menu), None);
    }

    #[test]
    fn turning_entries_on_and_off_leaves_nothing_behind() {
        let s = Scratch::new("onoff");
        let reg = s.registry();
        reg.set(Entry::Menu, true, Path::new(EXE), Language::It)
            .unwrap();
        assert_eq!(
            reg.state(),
            ShellState {
                menu: true,
                new_file: false
            }
        );
        assert_eq!(reg.choice(Entry::Menu), Some(true));
        let verb = s.class(r"Directory\Background\shell\PerMark").unwrap();
        assert_eq!(verb.get_value::<String, _>("").unwrap(), "Apri con PerMark");
        let command = s
            .class(r"SystemFileAssociations\.mkd\shell\PerMark\command")
            .unwrap();
        assert_eq!(
            command.get_value::<String, _>("").unwrap(),
            format!("\"{EXE}\" \"%1\"")
        );
        reg.set(Entry::NewFile, true, Path::new(EXE), Language::It)
            .unwrap();
        assert_eq!(
            reg.state(),
            ShellState {
                menu: true,
                new_file: true
            }
        );
        reg.set(Entry::Menu, false, Path::new(EXE), Language::It)
            .unwrap();
        assert_eq!(
            reg.state(),
            ShellState {
                menu: false,
                new_file: true
            }
        );
        assert_eq!(reg.choice(Entry::Menu), Some(false));
        for ext in [".md", ".markdown", ".mdown", ".mkd"] {
            assert!(
                s.class(&format!(r"SystemFileAssociations\{ext}")).is_none(),
                "{ext}: empty parents went"
            );
        }
        reg.set(Entry::NewFile, false, Path::new(EXE), Language::It)
            .unwrap();
        assert_nothing_under_directory(&s);
        // twice off and twice on are fine
        reg.set(Entry::NewFile, false, Path::new(EXE), Language::It)
            .unwrap();
        reg.set(Entry::Menu, true, Path::new(EXE), Language::En)
            .unwrap();
        reg.set(Entry::Menu, true, Path::new(EXE), Language::En)
            .unwrap();
        assert!(reg.state().menu);
    }

    #[test]
    fn a_parent_holding_anything_else_stays() {
        let s = Scratch::new("others");
        s.make(r"Directory\shell\Code", Some(("", "Open with Code")));
        s.make(
            r"SystemFileAssociations\.md",
            Some(("PerceivedType", "text")),
        );
        let reg = s.registry();
        reg.set(Entry::Menu, true, Path::new(EXE), Language::En)
            .unwrap();
        reg.set(Entry::Menu, false, Path::new(EXE), Language::En)
            .unwrap();
        assert!(s.class(r"Directory\shell\Code").is_some());
        let md = s.class(r"SystemFileAssociations\.md").unwrap();
        assert_eq!(md.get_value::<String, _>("PerceivedType").unwrap(), "text");
        assert!(s.class(r"SystemFileAssociations\.md\shell").is_none());
        assert!(s.class(r"SystemFileAssociations\.markdown").is_none());
    }

    #[test]
    fn relabel_rewords_only_what_is_there() {
        let s = Scratch::new("relabel");
        let reg = s.registry();
        assert!(!reg.relabel(Language::De).unwrap());
        reg.set(Entry::NewFile, true, Path::new(EXE), Language::En)
            .unwrap();
        assert!(reg.relabel(Language::ZhCn).unwrap());
        let verb = s.class(r"Directory\shell\PerMarkNew").unwrap();
        assert_eq!(
            verb.get_value::<String, _>("").unwrap(),
            "用 PerMark 新建 Markdown 文件"
        );
        assert!(
            s.class(r"Directory\shell\PerMark").is_none(),
            "nothing added"
        );
    }

    #[test]
    fn withdraw_removes_everything_and_records_off() {
        let s = Scratch::new("withdraw");
        let reg = s.registry();
        for entry in Entry::ALL {
            reg.set(entry, true, Path::new(EXE), Language::En).unwrap();
        }
        reg.withdraw(Path::new(EXE)).unwrap();
        assert_eq!(reg.state(), ShellState::default());
        assert_nothing_under_directory(&s);
        assert_eq!(reg.choice(Entry::Menu), Some(false));
        assert_eq!(reg.choice(Entry::NewFile), Some(false));
    }

    #[test]
    fn a_half_present_entry_reads_as_on_and_turning_it_off_clears_it() {
        let s = Scratch::new("half");
        s.make(
            r"Directory\Background\shell\PerMark\command",
            Some(("", "x")),
        );
        let reg = s.registry();
        assert!(reg.state().menu);
        reg.set(Entry::Menu, false, Path::new(EXE), Language::En)
            .unwrap();
        assert!(!reg.state().menu);
        assert_nothing_under_directory(&s);
    }
}
