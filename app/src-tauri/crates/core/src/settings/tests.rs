use std::fs;

use super::*;
use crate::paths::Root;
use crate::state::DEFAULT_CONFIG_TOML;

fn doc(text: &str) -> DocumentMut {
    text.parse().unwrap()
}

fn applied(text: &str, change: &SettingChange) -> String {
    let mut d = doc(text);
    apply_change(&mut d, change).unwrap();
    render(&d, text)
}

#[test]
fn image_folder_defaults_to_img_and_an_empty_value_stays_empty() {
    assert_eq!(AppSettings::from_doc(&doc("")).image_folder, "img");
    let set = applied("", &SettingChange::ImageFolder("pics".to_owned()));
    assert_eq!(AppSettings::from_doc(&doc(&set)).image_folder, "pics");
    let cleared = applied(&set, &SettingChange::ImageFolder(String::new()));
    assert!(cleared.contains("image_folder"));
    assert_eq!(AppSettings::from_doc(&doc(&cleared)).image_folder, "");
    assert_eq!(
        AppSettings::from_doc(&doc("image_folder = \"a/b\"\n")).image_folder,
        "img"
    );
    assert!(SettingChange::ImageFolder("a/b".to_owned())
        .validate()
        .is_err());
    assert!(SettingChange::ImageFolder("..".to_owned())
        .validate()
        .is_err());
    assert!(SettingChange::ImageFolder(String::new()).validate().is_ok());
    assert!(SettingChange::ImageFolder("  ".to_owned())
        .validate()
        .is_ok());
}

#[test]
fn defaults_when_empty() {
    let mut s = AppSettings::from_doc(&doc(""));
    s.characters = AppSettings::default().characters;
    assert_eq!(s, AppSettings::default());
    assert_eq!(
        (s.language, s.theme, s.tone, s.start_in),
        (
            LanguagePref::System,
            ThemePref::System,
            Tone::White,
            StartIn::Welcome
        )
    );
}

#[test]
fn every_choice_round_trips_through_the_file() {
    for &language in LanguagePref::ALL {
        let text = applied("", &SettingChange::Language(language));
        assert_eq!(AppSettings::from_doc(&doc(&text)).language, language);
    }
    for &tone in Tone::ALL {
        let text = applied("", &SettingChange::Tone(tone));
        assert_eq!(AppSettings::from_doc(&doc(&text)).tone, tone);
    }
    for &theme in ThemePref::ALL {
        assert_eq!(
            AppSettings::from_doc(&doc(&applied("", &SettingChange::Theme(theme)))).theme,
            theme
        );
    }
    for &start in StartIn::ALL {
        assert_eq!(
            AppSettings::from_doc(&doc(&applied("", &SettingChange::StartIn(start)))).start_in,
            start
        );
    }
}

#[test]
fn invalid_values_read_as_that_keys_default_and_others_survive() {
    let s = AppSettings::from_doc(&doc(
        "language = \"klingon\"\ntheme = 3\ntone = \"sage\"\nstart_in = [1]\nfuture = true\n",
    ));
    assert_eq!(s.language, LanguagePref::System);
    assert_eq!(s.theme, ThemePref::System);
    assert_eq!(s.tone, Tone::Sage);
}

#[test]
fn a_change_rewrites_only_that_value_keeping_comments_order_and_unknown_keys() {
    let before = "# my settings\r\ntheme = \"dark\"   # night owl\r\n\r\nlanguage = \"en\"\r\nfuture_key = { a = 1 }\r\n\r\n[plugins.git]\r\nenabled = true # yes\r\nbackend = \"system\"\r\n";
    let after = applied(before, &SettingChange::Language(LanguagePref::PtBr));
    assert_eq!(
        after,
        before.replace("language = \"en\"", "language = \"pt-BR\"")
    );
    let after = applied(before, &SettingChange::Theme(ThemePref::Light));
    assert_eq!(after, before.replace("\"dark\"", "\"light\""));
    let after = applied(
        before,
        &SettingChange::PluginEnabled {
            plugin: PluginId::of("git"),
            enabled: Some(false),
        },
    );
    assert_eq!(after, before.replace("enabled = true", "enabled = false"));
}

#[test]
fn plugin_switches_add_and_remove_cleanly() {
    let git = PluginId::of("git");
    let text = applied(
        "theme = \"dark\"\n",
        &SettingChange::PluginEnabled {
            plugin: git.clone(),
            enabled: Some(false),
        },
    );
    let s = AppSettings::from_doc(&doc(&text));
    assert_eq!(s.plugins.get(&git), Some(&false));
    assert!(text.starts_with("theme = \"dark\"\n"));
    let reset = applied(
        &text,
        &SettingChange::PluginEnabled {
            plugin: git.clone(),
            enabled: None,
        },
    );
    assert_eq!(AppSettings::from_doc(&doc(&reset)).plugins.len(), 0);
    assert!(!reset.contains("[plugins.git]"));
    // other plugin keys keep their table
    let kept = applied(
        "[plugins.git]\nenabled = true\nbackend = \"system\"\n",
        &SettingChange::PluginEnabled {
            plugin: git.clone(),
            enabled: None,
        },
    );
    assert_eq!(kept, "[plugins.git]\nbackend = \"system\"\n");
    // inline table form
    let inline = applied(
        "plugins = { git = { enabled = true } }\n",
        &SettingChange::PluginEnabled {
            plugin: git.clone(),
            enabled: Some(false),
        },
    );
    assert_eq!(
        AppSettings::from_doc(&doc(&inline)).plugins.get(&git),
        Some(&false)
    );
    // non-table `plugins` value replaced (it was invalid anyway)
    let fixed = applied(
        "plugins = 3\n",
        &SettingChange::PluginEnabled {
            plugin: git.clone(),
            enabled: Some(true),
        },
    );
    assert_eq!(
        AppSettings::from_doc(&doc(&fixed)).plugins.get(&git),
        Some(&true)
    );
    // reset of an absent plugin writes nothing
    assert_eq!(
        applied(
            "a = 1\n",
            &SettingChange::PluginEnabled {
                plugin: git,
                enabled: None
            }
        ),
        "a = 1\n"
    );
}

#[test]
fn invalid_plugin_ids_in_files_are_ignored() {
    let s = AppSettings::from_doc(&doc(
        "[plugins.\"Bad Id\"]\nenabled = true\n[plugins.git]\nenabled = \"yes\"\n",
    ));
    assert_eq!(s.plugins.len(), 0);
    assert!(s.plugin_tables.contains_key(&PluginId::of("git")));
}

#[test]
fn setting_change_wire_format() {
    let change: SettingChange =
        serde_json::from_str(r#"{"key":"language","value":"zh-CN"}"#).unwrap();
    assert_eq!(change, SettingChange::Language(LanguagePref::ZhCn));
    let change: SettingChange =
        serde_json::from_str(r#"{"key":"pluginEnabled","value":{"plugin":"git","enabled":null}}"#)
            .unwrap();
    assert_eq!(change.key(), "plugins.git.enabled");
    let change: SettingChange =
        serde_json::from_str(r#"{"key":"charactersFile","value":"chars.toml"}"#).unwrap();
    assert_eq!(
        change,
        SettingChange::CharactersFile("chars.toml".to_owned())
    );
    for bad in [
        r#"{"key":"language","value":"xx"}"#,
        r#"{"key":"pluginEnabled","value":{"plugin":"Bad","enabled":true}}"#,
        r#"{"key":"nope"}"#,
    ] {
        assert!(serde_json::from_str::<SettingChange>(bad).is_err(), "{bad}");
    }
}

#[test]
fn settings_file_apply_reads_fresh_and_keeps_hand_edits() {
    let base = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(base.path()));
    let mut got = file.load();
    got.characters_path = AppSettings::default().characters_path;
    assert_eq!(got, AppSettings::default());
    file.apply(&SettingChange::Theme(ThemePref::Dark)).unwrap();
    // hand edit between two changes
    let text = std::fs::read_to_string(file.path()).unwrap();
    std::fs::write(file.path(), format!("# hand\n{text}custom = 1\n")).unwrap();
    let s = file.apply(&SettingChange::Tone(Tone::Paper)).unwrap();
    assert_eq!((s.theme, s.tone), (ThemePref::Dark, Tone::Paper));
    let text = std::fs::read_to_string(file.path()).unwrap();
    assert!(text.starts_with("# hand\n") && text.contains("custom = 1"));
}

#[test]
fn a_settings_file_that_is_there_but_cannot_be_read_says_why_a_missing_one_does_not() {
    let base = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(base.path()));
    assert!(file.read_failure().is_none(), "no file yet");
    std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    std::fs::write(file.path(), "theme = \"dark\"\n").unwrap();
    assert!(file.read_failure().is_none());
    let latin1 = b"theme = \"dark\"\n# caf\xe9\n";
    std::fs::write(file.path(), latin1).unwrap();
    assert!(matches!(
        file.read_failure(),
        Some(SettingsError::Unreadable {
            why: Unreadable::NotText,
            ..
        })
    ));
    // the defaults run meanwhile, and the file stays as it is
    assert_eq!(file.load().theme, AppSettings::default().theme);
    assert_eq!(std::fs::read(file.path()).unwrap(), latin1);
}

#[test]
fn a_settings_file_that_is_not_utf8_or_too_large_is_refused_never_set_aside() {
    let base = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(base.path()));
    let dir = file.path().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    let names = || -> Vec<String> {
        std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    };
    let latin1 = b"theme = \"dark\"\n# caf\xe9\n".to_vec();
    std::fs::write(file.path(), &latin1).unwrap();
    assert_eq!(
        refusal(&file),
        ("notText", file.path().display().to_string())
    );
    assert_eq!(std::fs::read(file.path()).unwrap(), latin1);
    assert_eq!(names(), ["settings.toml"]);
    let mut big = b"theme = \"dark\"\n".to_vec();
    big.resize(usize::try_from(MAX_SETTINGS_BYTES).unwrap() + 1, b'\n');
    std::fs::write(file.path(), &big).unwrap();
    assert_eq!(
        refusal(&file),
        ("tooLarge", file.path().display().to_string())
    );
    assert_eq!(std::fs::read(file.path()).unwrap(), big);
    assert_eq!(names(), ["settings.toml"]);
}

/// Why a change to an unreadable settings file is refused, as the window hears it, and the file it is about.
fn refusal(file: &SettingsFile) -> (&'static str, String) {
    let error = IpcError::from(
        file.apply(&SettingChange::Theme(ThemePref::Light))
            .unwrap_err(),
    );
    assert_eq!(error.kind, crate::error::ErrorKind::SettingsUnreadable);
    let why = match error.detail.as_deref() {
        Some("notText") => "notText",
        Some("tooLarge") => "tooLarge",
        Some("locked") => "locked",
        other => panic!("{other:?}"),
    };
    (why, error.subject.unwrap_or_default())
}

#[cfg(windows)]
#[test]
fn a_settings_file_another_program_holds_is_refused_as_locked_and_left_alone() {
    use std::os::windows::fs::OpenOptionsExt;
    let base = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(base.path()));
    std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    std::fs::write(file.path(), "theme = \"dark\"\n").unwrap();
    // no sharing: every other open fails with a sharing violation, past the retries
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(file.path())
        .unwrap();
    assert_eq!(
        refusal(&file),
        ("locked", file.path().display().to_string())
    );
    drop(held);
    assert_eq!(
        std::fs::read_to_string(file.path()).unwrap(),
        "theme = \"dark\"\n"
    );
}

#[test]
fn a_corrupt_settings_file_is_kept_aside_not_overwritten() {
    let base = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(base.path()));
    std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    std::fs::write(file.path(), "theme = \"dark\n[[[").unwrap();
    let mut got = file.load();
    got.characters_path = AppSettings::default().characters_path;
    assert_eq!(got, AppSettings::default());
    file.apply(&SettingChange::Theme(ThemePref::Light)).unwrap();
    let names: Vec<String> = std::fs::read_dir(file.path().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().any(|n| n.starts_with("settings.toml.broken-")),
        "{names:?}"
    );
    assert_eq!(file.load().theme, ThemePref::Light);
}

#[test]
fn concurrent_changes_from_many_windows_all_land() {
    let base = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(base.path()));
    let lock = std::sync::Mutex::new(());
    std::thread::scope(|s| {
        for (i, &tone) in Tone::ALL.iter().enumerate() {
            let (file, lock) = (&file, &lock);
            s.spawn(move || {
                let _guard = lock.lock().unwrap();
                file.apply(&SettingChange::Tone(tone)).unwrap();
                file.apply(&SettingChange::PluginEnabled {
                    plugin: PluginId::parse(&format!("p{i}")).unwrap(),
                    enabled: Some(true),
                })
                .unwrap();
            });
        }
    });
    assert_eq!(file.load().plugins.len(), Tone::ALL.len());
}

#[test]
fn project_config_parses_leniently() {
    let c = ProjectConfig::from_doc(&doc(
        "ignore = [\"drafts/\", 3]\n[tree]\nshow = \"everything\"\ninclude = [\"*.sql\"]\nexclude = \"oops\"\n[agent]\ndeny = [\"notes\"]\n[plugins.word-count]\nenabled = false\n[plugins.git]\nenabled = true\n",
    ));
    assert_eq!(c.ignore, ["drafts/"]);
    assert_eq!(c.tree.show, Some(TreeShow::Everything));
    assert_eq!(c.tree.include, ["*.sql"]);
    assert_eq!(c.tree.exclude.len(), 0);
    assert!(c.agent_deny.contains("notes"));
    assert_eq!(
        c.plugins_off
            .iter()
            .map(PluginId::as_str)
            .collect::<Vec<_>>(),
        ["word-count"]
    );
    assert_eq!(
        ProjectConfig::from_doc(&doc(DEFAULT_CONFIG_TOML)),
        ProjectConfig::default()
    );
    assert_eq!(
        ProjectConfig::from_doc(&doc("[tree]\nshow = \"weird\"\n"))
            .tree
            .show,
        None
    );
}

#[test]
fn project_config_only_from_a_real_permark_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::open(dir.path()).unwrap();
    assert_eq!(ProjectConfig::load(&root), ProjectConfig::default());
    std::fs::create_dir(dir.path().join(".permark")).unwrap();
    std::fs::write(
        dir.path().join(".permark").join("config.toml"),
        "ignore = [\"x\"]\n",
    )
    .unwrap();
    assert_eq!(ProjectConfig::load(&root).ignore, ["x"]);
    std::fs::write(
        dir.path().join(".permark").join("config.toml"),
        "ignore = [\n",
    )
    .unwrap();
    assert_eq!(ProjectConfig::load(&root), ProjectConfig::default());
}

#[test]
fn reference_template_and_editor_round_trip_and_reset() {
    let defaults = AppSettings::from_doc(&doc(""));
    assert_eq!(defaults.reference_template, DEFAULT_REFERENCE_TEMPLATE);
    assert_eq!(defaults.editor, "");
    let text = applied(
        "",
        &SettingChange::ReferenceTemplate("@{path} ({section})".to_owned()),
    );
    let text = applied(
        &text,
        &SettingChange::Editor("code -g {path}:{line}:{col}".to_owned()),
    );
    let read = AppSettings::from_doc(&doc(&text));
    assert_eq!(read.reference_template, "@{path} ({section})");
    assert_eq!(read.editor, "code -g {path}:{line}:{col}");
    // empty text puts the key back to its default (the key leaves the file)
    let text = applied(&text, &SettingChange::Editor(String::new()));
    let text = applied(&text, &SettingChange::ReferenceTemplate("  ".to_owned()));
    assert!(
        !text.contains("editor") && !text.contains("reference_template"),
        "{text}"
    );
    let mut s = AppSettings::from_doc(&doc(&text));
    s.characters = AppSettings::default().characters;
    assert_eq!(s, AppSettings::default());
}

#[test]
fn bad_editor_templates_and_long_references_are_refused_before_they_reach_the_file() {
    for editor in [
        "code",
        "code \"{path}",
        "{path}",
        "code {file}",
        &format!("x {{path}} {}", "a".repeat(500)),
    ] {
        let change = SettingChange::Editor(editor.to_owned());
        assert!(
            matches!(change.validate(), Err(SettingsError::Invalid { .. })),
            "{editor}"
        );
    }
    assert!(SettingChange::Editor("zed {path}:{line}".to_owned())
        .validate()
        .is_ok());
    assert!(SettingChange::Editor(String::new()).validate().is_ok());
    let long = SettingChange::ReferenceTemplate("x".repeat(MAX_REFERENCE_TEMPLATE + 1));
    assert!(long.validate().is_err());
    let dir = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(dir.path()));
    let error = file
        .apply(&SettingChange::Editor("code".to_owned()))
        .unwrap_err();
    let ipc = IpcError::from(error);
    assert_eq!(ipc.kind, crate::error::ErrorKind::InvalidData);
    assert_eq!(ipc.subject.as_deref(), Some("editor"));
    assert_eq!(ipc.detail.as_deref(), Some("noPath"));
    assert!(!file.path().exists(), "a refused change writes nothing");
}

#[test]
fn hand_edited_editor_and_reference_values_that_are_not_valid_read_as_defaults() {
    let s = AppSettings::from_doc(&doc("editor = \"code\"\nreference_template = \"\"\n"));
    assert_eq!(s.editor, "");
    assert_eq!(s.reference_template, DEFAULT_REFERENCE_TEMPLATE);
    let s = AppSettings::from_doc(&doc(&format!(
        "reference_template = \"{}\"\neditor = 5\n",
        "x".repeat(300)
    )));
    assert_eq!(s.reference_template, DEFAULT_REFERENCE_TEMPLATE);
    assert_eq!(s.editor, "");
}

// ---- keyboard map ---------------------------------------------------------------------------------------------------

fn keymap_file(initial: &str) -> (tempfile::TempDir, SettingsFile) {
    let dir = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(dir.path()));
    if !initial.is_empty() {
        fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        fs::write(file.path(), initial).unwrap();
    }
    (dir, file)
}

fn bind(command: &str, combo: Option<&str>) -> SettingChange {
    SettingChange::Keymap {
        command: command.to_owned(),
        combo: combo.map(str::to_owned),
    }
}

#[test]
fn keymap_round_trips_through_the_file_and_only_its_bytes_change() {
    let original = "# mine\ntheme = \"dark\" # keep\n\n[plugins.git]\nenabled = true\n";
    let (_dir, file) = keymap_file(original);
    let s = file
        .apply(&bind("search.quickOpen", Some("Mod+Shift+K")))
        .unwrap();
    assert_eq!(
        s.keymap.get("search.quickOpen").map(String::as_str),
        Some("Mod+Shift+K")
    );
    let s = file.apply(&bind("doc.find", Some(""))).unwrap();
    assert_eq!(
        s.keymap.get("doc.find").map(String::as_str),
        Some(""),
        "unbound is kept"
    );
    let text = fs::read_to_string(file.path()).unwrap();
    assert!(
        text.starts_with("# mine\ntheme = \"dark\" # keep\n"),
        "{text}"
    );
    assert!(text.contains("[plugins.git]\nenabled = true"));
    assert!(
        text.contains("\"search.quickOpen\" = \"Mod+Shift+K\""),
        "{text}"
    );
    // back to the defaults removes the key, and the table with the last key
    file.apply(&bind("search.quickOpen", None)).unwrap();
    let s = file.apply(&bind("doc.find", None)).unwrap();
    assert!(s.keymap.is_empty());
    let after = fs::read_to_string(file.path()).unwrap();
    assert!(!after.contains("keymap"), "{after}");
    assert_eq!(after, original);
    // removing from a file with no table writes nothing new
    file.apply(&bind("never.there", None)).unwrap();
    assert_eq!(fs::read_to_string(file.path()).unwrap(), original);
}

#[test]
fn keymap_values_that_are_not_valid_are_refused_and_hand_edits_skipped() {
    for (command, combo) in [
        ("", "Mod+P"),
        ("has space", "Mod+P"),
        ("a/b", "Mod+P"),
        ("ok", "Mod + P"),
        ("ok", "Mod+\u{1}"),
        ("ok", "é"),
    ] {
        let change = bind(command, Some(combo));
        assert!(
            matches!(change.validate(), Err(SettingsError::Invalid { .. })),
            "{command:?} {combo:?}"
        );
    }
    assert!(bind(&"x".repeat(65), Some("Mod+P")).validate().is_err());
    assert!(bind("ok", Some(&"x".repeat(41))).validate().is_err());
    assert!(bind("ok", Some("")).validate().is_ok());
    assert!(bind("ok", None).validate().is_ok());
    let s = AppSettings::from_doc(&doc(
        "[keymap]\n\"good.one\" = \"Mod+P\"\n\"bad id\" = \"Mod+P\"\nnumber = 5\n\"x.y\" = \"two words\"\n",
    ));
    assert_eq!(
        s.keymap.keys().map(String::as_str).collect::<Vec<_>>(),
        ["good.one"]
    );
    let s = AppSettings::from_doc(&doc("keymap = 3\n"));
    assert!(s.keymap.is_empty());
    let many: String = std::iter::once("[keymap]\n".to_owned())
        .chain((0..500).map(|n| format!("c{n} = \"Mod+K\"\n")))
        .collect();
    assert_eq!(AppSettings::from_doc(&doc(&many)).keymap.len(), MAX_KEYMAP);
}

#[test]
fn the_git_backend_lives_in_the_plugin_table_and_default_leaves_nothing() {
    let before = "theme = \"dark\"\n\n[plugins.git]\nenabled = true\n";
    assert_eq!(
        AppSettings::from_doc(&doc(before)).git_backend,
        GitBackendPref::Automatic
    );
    for &choice in GitBackendPref::ALL {
        let text = applied(before, &SettingChange::GitBackend(choice));
        assert_eq!(AppSettings::from_doc(&doc(&text)).git_backend, choice);
        // the other keys of the table and the file stay
        assert!(text.contains("theme = \"dark\"") && text.contains("enabled = true"));
    }
    let system = applied(before, &SettingChange::GitBackend(GitBackendPref::System));
    assert_eq!(
        system,
        "theme = \"dark\"\n\n[plugins.git]\nenabled = true\nbackend = \"system\"\n"
    );
    // back to automatic removes the key (and an emptied table)
    assert_eq!(
        applied(
            &system,
            &SettingChange::GitBackend(GitBackendPref::Automatic)
        ),
        before
    );
    let only = applied("", &SettingChange::GitBackend(GitBackendPref::Builtin));
    assert_eq!(
        applied(&only, &SettingChange::GitBackend(GitBackendPref::Automatic)),
        ""
    );
    // a hand-written value that is not one of the three reads as the default
    assert_eq!(
        AppSettings::from_doc(&doc("[plugins.git]\nbackend = \"libgit\"\n")).git_backend,
        GitBackendPref::Automatic
    );
    assert_eq!(
        SettingChange::GitBackend(GitBackendPref::System).key(),
        "plugins.git.backend"
    );
}

#[test]
fn the_tokenizers_live_in_the_token_count_table_and_leave_nothing_when_cleared() {
    let before = "theme = \"dark\"\n\n[plugins.token-count]\nenabled = true\n";
    assert_eq!(
        AppSettings::from_doc(&doc(before)).tokenizers,
        Vec::<String>::new()
    );
    let chosen = applied(
        before,
        &SettingChange::Tokenizers(list(&["claude", "o200k"])),
    );
    assert_eq!(
        chosen,
        "theme = \"dark\"\n\n[plugins.token-count]\nenabled = true\ntokenizers = [\"claude\", \"o200k\"]\n"
    );
    assert_eq!(
        AppSettings::from_doc(&doc(&chosen)).tokenizers,
        list(&["claude", "o200k"])
    );
    // clearing removes the key and keeps the rest; on its own it removes the emptied table
    assert_eq!(
        applied(&chosen, &SettingChange::Tokenizers(Vec::new())),
        before
    );
    let only = applied("", &SettingChange::Tokenizers(list(&["chars4"])));
    assert_eq!(only, "[plugins.token-count]\ntokenizers = [\"chars4\"]\n");
    assert_eq!(applied(&only, &SettingChange::Tokenizers(Vec::new())), "");
    assert_eq!(
        SettingChange::Tokenizers(Vec::new()).key(),
        "plugins.token-count.tokenizers"
    );
}

#[test]
fn hand_written_tokenizers_are_read_leniently_and_written_strictly() {
    let read = |text: &str| AppSettings::from_doc(&doc(text)).tokenizers;
    // wrong type, wrong entries, repeats and the fourth are dropped
    assert_eq!(
        read("[plugins.token-count]\ntokenizers = \"o200k\"\n"),
        Vec::<String>::new()
    );
    assert_eq!(
        read("[plugins.token-count]\ntokenizers = [\"o200k\", 3, \"O200K\", \"o200k\", \"claude\", \"gemini\", \"chars4\"]\n"),
        list(&["o200k", "claude", "gemini"])
    );
    let too_long = "a".repeat(17);
    for bad in [
        list(&["a", "b", "c", "d"]),
        list(&["a", "a"]),
        list(&[""]),
        list(&["Upper"]),
        list(&["with space"]),
        list(&[&too_long]),
    ] {
        assert!(
            matches!(
                SettingChange::Tokenizers(bad.clone()).validate(),
                Err(SettingsError::Invalid { .. })
            ),
            "{bad:?}"
        );
        assert!(ProjectSettingChange::Tokenizers(bad).validate().is_err());
    }
    assert!(
        SettingChange::Tokenizers(list(&["o200k", "claude", "chars4"]))
            .validate()
            .is_ok()
    );
    assert!(SettingChange::Tokenizers(Vec::new()).validate().is_ok());
}

#[test]
fn the_notes_folder_is_a_plugin_key_changed_surgically() {
    let abs = if cfg!(windows) {
        r"D:\sync\My Notes"
    } else {
        "/srv/my notes"
    };
    let before =
        "# mine\r\ntheme = \"dark\"  # keep\r\n\r\n[plugins.git]\r\nbackend = \"system\"\r\n";
    let text = applied(before, &SettingChange::NotesFolder(abs.to_owned()));
    let read = AppSettings::from_doc(&doc(&text));
    assert_eq!(read.notes_folder, abs);
    assert_eq!(read.git_backend, GitBackendPref::System);
    assert!(
        text.starts_with("# mine\r\ntheme = \"dark\"  # keep\r\n"),
        "{text:?}"
    );
    assert!(
        text.contains("[plugins.git]\r\nbackend = \"system\"\r\n"),
        "{text:?}"
    );
    // empty = the default folder: the key goes, nothing else
    let back = applied(&text, &SettingChange::NotesFolder(String::new()));
    assert_eq!(AppSettings::from_doc(&doc(&back)).notes_folder, "");
    assert_eq!(back.replace("[plugins.notes]\r\n", ""), before);
    assert_eq!(
        SettingChange::NotesFolder(String::new()).key(),
        "plugins.notes.folder"
    );
    // hand edits that are not a folder read as the default
    for hand in ["relative/notes", ""] {
        let text = format!("[plugins.notes]\nfolder = \"{hand}\"\n");
        assert_eq!(
            AppSettings::from_doc(&doc(&text)).notes_folder,
            "",
            "{hand}"
        );
    }
    let wrong = "[plugins.notes]\nfolder = 3\n";
    assert_eq!(AppSettings::from_doc(&doc(wrong)).notes_folder, "");
}

#[test]
fn a_notes_folder_must_be_absolute_and_sane() {
    assert!(SettingChange::NotesFolder(String::new()).validate().is_ok());
    let abs = if cfg!(windows) { r"C:\notes" } else { "/notes" };
    assert!(SettingChange::NotesFolder(abs.to_owned())
        .validate()
        .is_ok());
    for bad in ["notes", "./notes", r"..\x", "a\0b", &"x".repeat(2000)] {
        assert!(
            SettingChange::NotesFolder(bad.to_owned())
                .validate()
                .is_err(),
            "{bad:?}"
        );
    }
}

// ---- open_in, installer language, project settings writes --------------------------------------------------------------

#[test]
fn open_in_round_trips_and_an_invalid_value_reads_as_tab() {
    for &choice in OpenIn::ALL {
        let text = applied("", &SettingChange::OpenIn(choice));
        assert_eq!(AppSettings::from_doc(&doc(&text)).open_in, choice);
    }
    assert_eq!(
        AppSettings::from_doc(&doc("open_in = \"tabs\"\n")).open_in,
        OpenIn::Tab
    );
    assert_eq!(
        AppSettings::from_doc(&doc("open_in = 3\n")).open_in,
        OpenIn::Tab
    );
    assert_eq!(SettingChange::OpenIn(OpenIn::NewWindow).key(), "open_in");
}

#[test]
fn open_in_values_of_0_6_0_map_on_read_and_the_file_changes_only_when_set() {
    for (old, now) in [
        ("ask", OpenIn::Tab),
        ("this", OpenIn::ThisTab),
        ("new", OpenIn::NewWindow),
    ] {
        let text = format!("# mine\nopen_in = \"{old}\" # kept\ntheme = \"dark\"\n");
        assert_eq!(AppSettings::from_doc(&doc(&text)).open_in, now, "{old}");
        // another setting changes: the old value stays as written
        let other = applied(&text, &SettingChange::EditMode(EditModePref::Source));
        assert!(
            other.contains(&format!("open_in = \"{old}\" # kept")),
            "{other}"
        );
        // the person sets it: only that value is rewritten
        let set = applied(&text, &SettingChange::OpenIn(OpenIn::NewWindow));
        assert_eq!(
            set,
            text.replace(&format!("\"{old}\""), "\"window\""),
            "{old}"
        );
    }
    assert_eq!(AppSettings::from_doc(&doc("")).open_in, OpenIn::Tab);
}

#[test]
fn wrap_lines_is_on_unless_the_file_says_false() {
    // a settings file of 0.6.1 (before the key existed): long lines wrap
    let old = AppSettings::from_doc(&doc(include_str!(
        "../../../../src/fixtures/settings-0.6.1.toml"
    )));
    assert!(old.wrap_lines);
    assert!(AppSettings::from_doc(&doc("")).wrap_lines);
    assert!(!AppSettings::from_doc(&doc("wrap_lines = false\n")).wrap_lines);
    // a value that is not a boolean counts as not set
    assert!(AppSettings::from_doc(&doc("wrap_lines = \"no\"\n")).wrap_lines);
    assert!(AppSettings::from_doc(&doc("wrap_lines = 0\n")).wrap_lines);
    assert_eq!(SettingChange::WrapLines(false).key(), "wrap_lines");
}

#[test]
fn wrap_lines_change_rewrites_only_its_key() {
    let text = "# mine\ntheme = \"dark\" # kept\n\n[plugins.git]\nenabled = false\n";
    let off = applied(text, &SettingChange::WrapLines(false));
    // one line more, every other line as it was
    let rest: Vec<&str> = off.lines().filter(|l| *l != "wrap_lines = false").collect();
    assert_eq!(rest, text.lines().collect::<Vec<_>>(), "{off}");
    assert!(!AppSettings::from_doc(&doc(&off)).wrap_lines);
    assert_eq!(AppSettings::from_doc(&doc(&off)).theme, ThemePref::Dark);
    let written = "# mine\nwrap_lines = false # mine too\ntheme = \"dark\"\n";
    let on = applied(written, &SettingChange::WrapLines(true));
    assert_eq!(on, written.replace("false", "true"));
    assert!(AppSettings::from_doc(&doc(&on)).wrap_lines);
}

#[test]
fn keymap_entries_of_0_6_0_read_under_the_new_ids() {
    let text = "[keymap]\n\"app.closeWorkspace\" = \"Mod+Q\"\n\"workspace.next\" = \"Mod+Alt+N\"\n\"tab.next\" = \"Mod+J\"\n";
    let s = AppSettings::from_doc(&doc(text));
    assert_eq!(s.keymap.get("tab.close").map(String::as_str), Some("Mod+Q"));
    assert_eq!(
        s.keymap.get("tab.next").map(String::as_str),
        Some("Mod+J"),
        "new id wins"
    );
    assert!(!s.keymap.contains_key("app.closeWorkspace"));
    assert!(!s.keymap.contains_key("workspace.next"));
    // changing it rewrites that entry under the new id; other entries untouched
    let set = applied(
        text,
        &SettingChange::Keymap {
            command: "tab.close".to_owned(),
            combo: Some("Mod+W".to_owned()),
        },
    );
    assert!(!set.contains("app.closeWorkspace"), "{set}");
    assert!(set.contains("\"workspace.next\" = \"Mod+Alt+N\""), "{set}");
    assert_eq!(
        AppSettings::from_doc(&doc(&set))
            .keymap
            .get("tab.close")
            .map(String::as_str),
        Some("Mod+W")
    );
}

#[test]
fn installer_codes_are_one_of_the_thirteen_and_nothing_else() {
    use super::installer::language_from_code;
    assert_eq!(language_from_code("en"), Some(LanguagePref::En));
    assert_eq!(language_from_code(" pt-BR\r\n"), Some(LanguagePref::PtBr));
    assert_eq!(language_from_code("zh-CN"), Some(LanguagePref::ZhCn));
    assert_eq!(language_from_code("ja"), Some(LanguagePref::Ja));
    assert_eq!(language_from_code("ja-JP"), None);
    assert_eq!(language_from_code("ko"), Some(LanguagePref::Ko));
    assert_eq!(language_from_code("fr"), Some(LanguagePref::Fr));
    assert_eq!(language_from_code("pt-PT"), Some(LanguagePref::PtPt));
    assert_eq!(language_from_code("ca"), Some(LanguagePref::Ca));
    assert_eq!(language_from_code("gl"), Some(LanguagePref::Gl));
    assert_eq!(language_from_code("ro"), Some(LanguagePref::Ro));
    assert_eq!(
        language_from_code("de\0"),
        None,
        "a stray NUL is not a code"
    );
    for other in ["", "system", "klingon", "EN", "pt", "zh-TW", "en,de"] {
        assert_eq!(language_from_code(other), None, "{other:?}");
    }
}

fn settings_file(dir: &Path) -> SettingsFile {
    let dirs = crate::state::AppDirs::under(dir);
    fs::create_dir_all(&dirs.profile).unwrap();
    SettingsFile::app(&dirs)
}

#[test]
fn the_installer_language_is_adopted_once_and_never_over_a_choice() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    // nothing recorded: nothing written
    assert!(file.adopt_language(None).unwrap().is_none());
    assert!(!file.path().exists());
    // first start: written
    let first = file
        .adopt_language(Some(LanguagePref::De))
        .unwrap()
        .unwrap();
    assert_eq!(first.language, LanguagePref::De);
    assert_eq!(file.load().language, LanguagePref::De);
    // a later reinstall in another language never replaces it
    assert!(file
        .adopt_language(Some(LanguagePref::It))
        .unwrap()
        .is_none());
    assert_eq!(file.load().language, LanguagePref::De);
    // the person picks "system": that is a choice too
    file.apply(&SettingChange::Language(LanguagePref::System))
        .unwrap();
    assert!(file
        .adopt_language(Some(LanguagePref::En))
        .unwrap()
        .is_none());
    assert_eq!(file.load().language, LanguagePref::System);
}

#[test]
fn the_installer_language_keeps_other_keys_and_a_corrupt_file_is_set_aside() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    fs::write(file.path(), "# mine\r\ntheme = \"dark\" # night\r\n").unwrap();
    file.adopt_language(Some(LanguagePref::Es))
        .unwrap()
        .unwrap();
    let text = fs::read_to_string(file.path()).unwrap();
    assert!(text.contains("theme = \"dark\" # night\r\n"), "{text:?}");
    assert!(text.contains("# mine\r\n") && !text.replace("\r\n", "").contains('\n'));
    assert_eq!(file.load().language, LanguagePref::Es);
    // not TOML: moved aside, defaults + the language written
    fs::write(file.path(), "language = [[[").unwrap();
    file.adopt_language(Some(LanguagePref::It))
        .unwrap()
        .unwrap();
    assert_eq!(file.load().language, LanguagePref::It);
    let backups = fs::read_dir(file.path().parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".broken-"))
        .count();
    assert_eq!(backups, 1);
}

fn project_applied(text: &str, change: &ProjectSettingChange) -> String {
    let mut d = doc(text);
    project::apply_project_change(&mut d, change);
    render(&d, text)
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn every_project_setting_round_trips_through_the_file() {
    let mut text = DEFAULT_CONFIG_TOML.to_owned();
    for change in [
        ProjectSettingChange::TreeShow(Some(TreeShow::DocsAndAssets)),
        ProjectSettingChange::TreeInclude(list(&["*.sql", "db/**"])),
        ProjectSettingChange::TreeExclude(list(&["archive/**"])),
        ProjectSettingChange::Ignore(list(&["drafts/", "*.generated.md"])),
        ProjectSettingChange::AgentDeny(list(&["notes", "comments.resolve"])),
        ProjectSettingChange::AgentDenyPaths(list(&["secret/**"])),
        ProjectSettingChange::Tokenizers(list(&["claude", "o200k"])),
    ] {
        text = project_applied(&text, &change);
    }
    let config = ProjectConfig::from_doc(&doc(&text));
    assert_eq!(config.tree.show, Some(TreeShow::DocsAndAssets));
    assert_eq!(config.tree.include, list(&["*.sql", "db/**"]));
    assert_eq!(config.tree.exclude, list(&["archive/**"]));
    assert_eq!(config.ignore, list(&["drafts/", "*.generated.md"]));
    assert!(config.agent_deny.contains("notes") && config.agent_deny.contains("comments.resolve"));
    assert_eq!(config.agent_deny_paths, list(&["secret/**"]));
    assert_eq!(config.tokenizers, list(&["claude", "o200k"]));
    // the commented defaults are still explaining the file
    assert!(text.contains("# PerMark project settings."));
    assert!(text.contains("# Turn a plugin off for everyone in this project."));
}

#[test]
fn removing_every_project_setting_gives_the_file_back_byte_for_byte() {
    let mut text = DEFAULT_CONFIG_TOML.to_owned();
    let changes = [
        ProjectSettingChange::TreeShow(Some(TreeShow::Custom)),
        ProjectSettingChange::TreeInclude(list(&["a"])),
        ProjectSettingChange::TreeExclude(list(&["b"])),
        ProjectSettingChange::Ignore(list(&["c"])),
        ProjectSettingChange::AgentDeny(list(&["notes"])),
        ProjectSettingChange::AgentDenyPaths(list(&["d"])),
        ProjectSettingChange::Tokenizers(list(&["gemini"])),
    ];
    for change in &changes {
        text = project_applied(&text, change);
    }
    assert_ne!(text, DEFAULT_CONFIG_TOML);
    let clears = [
        ProjectSettingChange::TreeShow(None),
        ProjectSettingChange::TreeInclude(Vec::new()),
        ProjectSettingChange::TreeExclude(Vec::new()),
        ProjectSettingChange::Ignore(Vec::new()),
        ProjectSettingChange::AgentDeny(Vec::new()),
        ProjectSettingChange::AgentDenyPaths(Vec::new()),
        ProjectSettingChange::Tokenizers(Vec::new()),
    ];
    for change in &clears {
        text = project_applied(&text, change);
    }
    assert_eq!(text, DEFAULT_CONFIG_TOML, "nothing of ours is left behind");
}

#[test]
fn a_new_table_goes_below_the_header_and_the_header_stays_when_the_table_goes() {
    let show = ProjectSettingChange::TreeShow(Some(TreeShow::Everything));
    // the file "Initialize PerMark" writes: comments only
    let out = project_applied(DEFAULT_CONFIG_TOML, &show);
    assert_eq!(
        out,
        format!("{DEFAULT_CONFIG_TOML}[tree]\nshow = \"everything\"\n")
    );
    let crlf = DEFAULT_CONFIG_TOML.replace('\n', "\r\n");
    let out = project_applied(&crlf, &show);
    assert_eq!(out, format!("{crlf}[tree]\r\nshow = \"everything\"\r\n"));
    // a hand-written header above the first table stays when that table goes
    let hand =
        "# team file\n\n[tree]\nshow = \"documents\" # default\n\n[agent]\ndeny = [\"notes\"]\n";
    let out = project_applied(hand, &ProjectSettingChange::TreeShow(None));
    // (the blank line above a table goes with it)
    assert_eq!(out, "# team file\n\n[agent]\ndeny = [\"notes\"]\n");
    let out = project_applied(&out, &ProjectSettingChange::AgentDeny(Vec::new()));
    assert_eq!(out, "# team file\n");
    // a root key below the header, and a last comment line with no line end
    let out = project_applied(
        "# head\n# no end",
        &ProjectSettingChange::Ignore(list(&["x/"])),
    );
    assert!(out.starts_with("# head\n"), "{out:?}");
    assert!(out.contains("ignore = [\"x/\"]"), "{out:?}");
    assert_eq!(ProjectConfig::from_doc(&doc(&out)).ignore, list(&["x/"]));
}

#[test]
fn a_project_write_keeps_hand_edits_comments_unknown_keys_and_crlf() {
    let hand = "# team file\r\nignore = [\"old/\"] # kept for CI\r\nfuture = 1\r\n\r\n[tree]\r\nshow = \"documents\" # default\r\n\r\n[plugins.word-count]\r\nenabled = false\r\n";
    let out = project_applied(
        hand,
        &ProjectSettingChange::TreeShow(Some(TreeShow::Everything)),
    );
    assert_eq!(
        out,
        hand.replace("\"documents\" # default", "\"everything\" # default")
    );
    let out = project_applied(hand, &ProjectSettingChange::Ignore(list(&["new/"])));
    assert!(
        out.contains("ignore = [\"new/\"] # kept for CI\r\n"),
        "{out:?}"
    );
    assert!(out.contains("future = 1") && out.contains("enabled = false"));
    assert!(!out.replace("\r\n", "").contains('\n'), "CRLF kept");
}

#[test]
fn project_lists_are_trimmed_and_deduplicated_and_garbage_is_refused() {
    let out = project_applied(
        "",
        &ProjectSettingChange::Ignore(list(&[" a/ ", "a/", "b"])),
    );
    assert_eq!(
        ProjectConfig::from_doc(&doc(&out)).ignore,
        list(&["a/", "b"])
    );
    let many: Vec<String> = (0..201).map(|n| format!("p{n}")).collect();
    let refuse = |change: ProjectSettingChange, reason: &str| match change.validate() {
        Err(SettingsError::Invalid { reason: got, .. }) => assert_eq!(got, reason),
        other => panic!("{other:?}"),
    };
    refuse(ProjectSettingChange::Ignore(many), "tooMany");
    refuse(ProjectSettingChange::Ignore(list(&["a", "  "])), "empty");
    refuse(
        ProjectSettingChange::Ignore(vec!["x".repeat(301)]),
        "tooLong",
    );
    refuse(ProjectSettingChange::Ignore(list(&["a\nb"])), "control");
    refuse(ProjectSettingChange::Ignore(list(&["a\0"])), "control");
    refuse(ProjectSettingChange::AgentDeny(list(&["Notes"])), "group");
    refuse(ProjectSettingChange::AgentDeny(list(&["a.b.c"])), "group");
    refuse(
        ProjectSettingChange::AgentDeny(list(&["comments."])),
        "group",
    );
    refuse(
        ProjectSettingChange::AgentDeny(vec!["a".repeat(33)]),
        "group",
    );
    assert!(
        ProjectSettingChange::AgentDeny(list(&["comments.resolve", "show"]))
            .validate()
            .is_ok()
    );
    // exactly 200 entries of 300 characters are fine
    let edge: Vec<String> = (0..200).map(|n| format!("{n:0>300}")).collect();
    assert!(ProjectSettingChange::AgentDenyPaths(edge)
        .validate()
        .is_ok());
}

#[test]
fn the_project_file_is_written_only_inside_a_real_permark_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::open(dir.path()).unwrap();
    let file = ProjectConfigFile::of(&root);
    let change = ProjectSettingChange::Ignore(list(&["x/"]));
    assert!(matches!(
        file.apply(&change),
        Err(SettingsError::NotInitialized)
    ));
    assert!(!dir.path().join(".permark").exists());
    // `.permark` is a file: not initialized, nothing replaced
    fs::write(dir.path().join(".permark"), "not a folder").unwrap();
    assert!(matches!(
        file.apply(&change),
        Err(SettingsError::NotInitialized)
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join(".permark")).unwrap(),
        "not a folder"
    );
    fs::remove_file(dir.path().join(".permark")).unwrap();
    // a real folder without a config: the file is made
    fs::create_dir(dir.path().join(".permark")).unwrap();
    assert!(file.initialized());
    let config = file.apply(&change).unwrap();
    assert_eq!(config.ignore, list(&["x/"]));
    assert_eq!(
        ProjectConfig::load(&root).ignore,
        list(&["x/"]),
        "the file on disk says it"
    );
    // a hand-broken file is set aside, the setting is still written
    fs::write(dir.path().join(".permark/config.toml"), "ignore = [[[").unwrap();
    file.apply(&change).unwrap();
    assert_eq!(ProjectConfig::load(&root).ignore, list(&["x/"]));
    let aside = fs::read_dir(dir.path().join(".permark"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".broken-"))
        .count();
    assert_eq!(aside, 1);
}

#[test]
fn a_project_config_over_a_unicode_and_space_path_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("meu projeto — 项目");
    fs::create_dir_all(folder.join(".permark")).unwrap();
    let root = Root::open(&folder).unwrap();
    let file = ProjectConfigFile::of(&root);
    file.apply(&ProjectSettingChange::AgentDenyPaths(list(&["ação/**"])))
        .unwrap();
    assert_eq!(
        ProjectConfig::load(&root).agent_deny_paths,
        list(&["ação/**"])
    );
}

#[cfg(unix)]
#[test]
fn a_config_file_that_is_a_link_is_not_written_through() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::open(dir.path()).unwrap();
    fs::create_dir(dir.path().join(".permark")).unwrap();
    let outside = dir.path().join("outside.toml");
    fs::write(&outside, "ignore = [\"keep/\"]\n").unwrap();
    std::os::unix::fs::symlink(&outside, dir.path().join(".permark/config.toml")).unwrap();
    let file = ProjectConfigFile::of(&root);
    let refused = file.apply(&ProjectSettingChange::Ignore(list(&["x/"])));
    assert!(matches!(refused, Err(SettingsError::Invalid { .. })));
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "ignore = [\"keep/\"]\n"
    );
}

#[test]
fn the_choices_0_5_kept_are_adopted_once_and_bad_values_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    let notes = dir.path().join("My Notes ü");
    let older = dir.path().join("settings.json");
    fs::write(
        &older,
        serde_json::json!({
            "notesFolder": notes.to_string_lossy(),
            "editorCommand": "code -g {path}:{line}:{col}",
            "language": "pt-BR",
            "tone": "sage",
            "other": 1
        })
        .to_string(),
    )
    .unwrap();
    let adopted = file.adopt_older(&older).unwrap().unwrap();
    assert_eq!(adopted.notes_folder, notes.to_string_lossy());
    assert_eq!(adopted.language, LanguagePref::PtBr);
    assert_eq!(file.load().notes_folder, notes.to_string_lossy());
    assert!(older.is_file(), "0.5's file is only read");
    // once: this version's settings exist now
    fs::write(&older, r#"{"language":"de"}"#).unwrap();
    assert!(file.adopt_older(&older).unwrap().is_none());
    assert_eq!(file.load().language, LanguagePref::PtBr);
    // the installer's language never replaces the one 0.5 had
    assert!(file
        .adopt_language(Some(LanguagePref::It))
        .unwrap()
        .is_none());
}

#[test]
fn older_settings_that_are_missing_damaged_or_odd_write_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    let older = dir.path().join("settings.json");
    assert!(file.adopt_older(&older).unwrap().is_none());
    for text in [
        "not json",
        "[1, 2]",
        r#"{"notesFolder": "relative/folder", "language": "xx", "editorCommand": 5}"#,
        r#"{"notesFolder": "   "}"#,
    ] {
        fs::write(&older, text).unwrap();
        assert!(file.adopt_older(&older).unwrap().is_none(), "{text}");
        assert!(!file.path().exists(), "{text}");
    }
}

// ---- right_views (design-live-follow §14.2.1) ------------------------------------------------------------------

fn stacked() -> RightViews {
    RightViews {
        v: 1,
        groups: vec![
            ViewGroup {
                views: vec!["outline".to_owned()],
                share: 0.5,
            },
            ViewGroup {
                views: vec!["live-feed.panel".to_owned()],
                share: 0.5,
            },
        ],
    }
}

/// A settings file 0.6.4 wrote (no `[right_views]`): reads as `None` (the frontend's default), and setting the
/// arrangement adds only its table.
const SETTINGS_0_6_4: &str = "# my settings\r\nlanguage = \"pt-BR\"\r\ntheme = \"dark\" # night\r\nopen_in = \"tab\"\r\n\r\n[keymap]\r\n\"tab.close\" = \"Mod+W\"\r\n\r\n[plugins.git]\r\nbackend = \"system\"\r\n";

#[test]
fn a_0_6_4_settings_file_has_no_arrangement_and_gains_only_its_table() {
    let old = AppSettings::from_doc(&doc(SETTINGS_0_6_4));
    assert_eq!(old.right_views, None);
    assert_eq!(old.language, LanguagePref::PtBr);
    let text = applied(SETTINGS_0_6_4, &SettingChange::RightViews(Some(stacked())));
    assert!(text.starts_with(SETTINGS_0_6_4), "{text}");
    assert!(text.contains("\r\n[right_views]\r\n"), "{text}");
    assert!(
        !text.replace("\r\n", "").contains('\n'),
        "CRLF kept: {text:?}"
    );
    let read = AppSettings::from_doc(&doc(&text));
    assert_eq!(read.right_views, Some(stacked()));
    assert_eq!(read.theme, ThemePref::Dark);
    // back to the default: the table goes, the file is as 0.6.4 left it
    assert_eq!(
        applied(&text, &SettingChange::RightViews(None)),
        SETTINGS_0_6_4
    );
    assert_eq!(
        applied(SETTINGS_0_6_4, &SettingChange::RightViews(None)),
        SETTINGS_0_6_4
    );
}

#[test]
fn a_changed_arrangement_keeps_its_place_and_the_rest_of_the_file() {
    let text = "theme = \"light\"\n\n# the right column\n[right_views]\nv = 1\ngroups = [{ views = [\"outline\"], share = 1 }]\n\n[keymap]\n\"tab.close\" = \"Mod+W\"\n";
    let mut tabs = stacked();
    tabs.groups = vec![ViewGroup {
        views: vec!["outline".to_owned(), "live-feed.panel".to_owned()],
        share: 1.0,
    }];
    let read = AppSettings::from_doc(&doc(text));
    // a whole number share (hand-written) reads as a fraction
    assert_eq!(
        read.right_views.as_ref().unwrap().groups[0].views,
        ["outline"]
    );
    let out = applied(text, &SettingChange::RightViews(Some(tabs.clone())));
    assert!(
        out.starts_with("theme = \"light\"\n\n# the right column\n[right_views]\n"),
        "{out}"
    );
    assert!(
        out.ends_with("[keymap]\n\"tab.close\" = \"Mod+W\"\n"),
        "{out}"
    );
    assert_eq!(AppSettings::from_doc(&doc(&out)).right_views, Some(tabs));
}

#[test]
fn an_unreadable_arrangement_reads_as_the_default_and_the_rest_survives() {
    for bad in [
        "[right_views]\ngroups = []\n",
        "[right_views]\nv = -1\ngroups = []\n",
        "[right_views]\nv = \"1\"\ngroups = []\n",
        "[right_views]\nv = 1\n",
        "[right_views]\nv = 1\ngroups = \"outline\"\n",
        "[right_views]\nv = 1\ngroups = [1, 2]\n",
        "[right_views]\nv = 1\ngroups = [{ views = \"outline\", share = 0.5 }]\n",
        "[right_views]\nv = 1\ngroups = [{ views = [1], share = 0.5 }]\n",
        "[right_views]\nv = 1\ngroups = [{ views = [\"outline\"] }]\n",
        "[right_views]\nv = 1\ngroups = [{ views = [\"outline\"], share = \"half\" }]\n",
        "[right_views]\nv = 1\ngroups = [{ views = [\"outline\"], share = nan }]\n",
        "[right_views]\nv = 1\ngroups = [{ views = [\"outline\"], share = inf }]\n",
        "right_views = 3\n",
    ] {
        let text = format!("theme = \"dark\"\n{bad}");
        let read = AppSettings::from_doc(&doc(&text));
        assert_eq!(read.right_views, None, "{bad}");
        assert_eq!(read.theme, ThemePref::Dark, "{bad}");
    }
    // the other spelling of an array of tables reads too; an unknown key inside is ignored
    let text = "[right_views]\nv = 2\nmine = true\n\n[[right_views.groups]]\nviews = [\"outline\", \"x:y\"]\nshare = 0.25\n";
    assert_eq!(
        AppSettings::from_doc(&doc(text)).right_views,
        Some(RightViews {
            v: 2,
            groups: vec![ViewGroup {
                views: vec!["outline".to_owned(), "x:y".to_owned()],
                share: 0.25,
            }],
        })
    );
}

#[test]
fn an_arrangement_out_of_bounds_is_refused_before_it_reaches_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    fs::write(file.path(), "theme = \"dark\"\n").unwrap();
    let group = |views: &[&str], share: f64| ViewGroup {
        views: views.iter().map(|v| (*v).to_owned()).collect(),
        share,
    };
    let long = "x".repeat(65);
    let many: Vec<&str> = vec!["a"; 17];
    for groups in [
        vec![group(&["a"], 0.25); 4],
        vec![group(&many, 1.0)],
        vec![group(&[""], 1.0)],
        vec![group(&[long.as_str()], 1.0)],
        vec![group(&["a b"], 1.0)],
        vec![group(&["ü"], 1.0)],
        vec![group(&["a"], 1.5)],
        vec![group(&["a"], -0.1)],
        vec![group(&["a"], f64::NAN)],
        vec![group(&["a"], f64::INFINITY)],
    ] {
        let change = SettingChange::RightViews(Some(RightViews { v: 1, groups }));
        assert!(
            matches!(file.apply(&change), Err(SettingsError::Invalid { ref reason, .. }) if reason == "rightViews"),
            "{change:?}"
        );
    }
    assert_eq!(
        fs::read_to_string(file.path()).unwrap(),
        "theme = \"dark\"\n"
    );
    // in bounds: written; an empty arrangement is a value too (the frontend decides what it means)
    let saved = file
        .apply(&SettingChange::RightViews(Some(stacked())))
        .unwrap();
    assert_eq!(saved.right_views, Some(stacked()));
    assert_eq!(file.load().right_views, Some(stacked()));
    let empty = RightViews {
        v: 1,
        groups: Vec::new(),
    };
    file.apply(&SettingChange::RightViews(Some(empty.clone())))
        .unwrap();
    assert_eq!(file.load().right_views, Some(empty));
}

#[test]
fn the_arrangement_change_wire_format() {
    let change: SettingChange = serde_json::from_str(
        r#"{"key":"rightViews","value":{"v":1,"groups":[{"views":["outline"],"share":0.5},{"views":["live-feed.panel"],"share":0.5}]}}"#,
    )
    .unwrap();
    assert_eq!(change, SettingChange::RightViews(Some(stacked())));
    assert_eq!(change.key(), "right_views");
    let reset: SettingChange =
        serde_json::from_str(r#"{"key":"rightViews","value":null}"#).unwrap();
    assert_eq!(reset, SettingChange::RightViews(None));
    let settings = serde_json::to_value(AppSettings::default()).unwrap();
    assert_eq!(settings["rightViews"], serde_json::Value::Null);
    for bad in [
        r#"{"key":"rightViews","value":{"groups":[]}}"#,
        r#"{"key":"rightViews","value":{"v":1,"groups":[{"views":"outline","share":0.5}]}}"#,
    ] {
        assert!(serde_json::from_str::<SettingChange>(bad).is_err(), "{bad}");
    }
    // no value at all = the reset (serde reads an absent `Option` content as `None`)
    assert_eq!(
        serde_json::from_str::<SettingChange>(r#"{"key":"rightViews"}"#).unwrap(),
        SettingChange::RightViews(None)
    );
}

/// The exact bytes of a written `[right_views]`: its groups go through the writer the regions of `[panels]` share.
#[test]
fn the_arrangement_table_is_written_in_one_shape() {
    assert_eq!(
        applied(
            "theme = \"dark\"\n",
            &SettingChange::RightViews(Some(stacked()))
        ),
        "theme = \"dark\"\n\n[right_views]\nv = 1\ngroups = [{ views = [\"outline\"], share = 0.5 }, { views = [\"live-feed.panel\"], share = 0.5 }]\n"
    );
}

// ---- panels + layouts: the arrangement of every region, and the named arrangements -----------------------------

fn view_group(views: &[&str], share: f64) -> ViewGroup {
    ViewGroup {
        views: views.iter().map(|v| (*v).to_owned()).collect(),
        share,
    }
}

fn docked() -> Panels {
    Panels {
        v: 2,
        left: vec![view_group(&["files", "search"], 1.0)],
        right: vec![
            view_group(&["outline"], 0.5),
            view_group(&["live-feed.panel"], 0.5),
        ],
        bottom: Vec::new(),
    }
}

fn no_panels() -> Panels {
    Panels {
        v: 2,
        left: Vec::new(),
        right: Vec::new(),
        bottom: Vec::new(),
    }
}

fn named() -> Layouts {
    Layouts {
        v: 1,
        items: vec![
            LayoutItem {
                name: "Writing".to_owned(),
                arrangement: docked(),
            },
            LayoutItem {
                name: "Revisão ampla".to_owned(),
                arrangement: Panels {
                    bottom: vec![view_group(&["outline", "files"], 1.0)],
                    ..no_panels()
                },
            },
        ],
    }
}

/// `[panels]` as `docked()` is written.
const DOCKED_TOML: &str = "[panels]\nv = 2\nleft = [{ views = [\"files\", \"search\"], share = 1.0 }]\nright = [{ views = [\"outline\"], share = 0.5 }, { views = [\"live-feed.panel\"], share = 0.5 }]\nbottom = []\n";

/// `[layouts]` as `named()` is written: one `[[layouts.items]]` per layout, its arrangement in a table below it.
const NAMED_TOML: &str = "[layouts]\nv = 1\n\n[[layouts.items]]\nname = \"Writing\"\n\n[layouts.items.arrangement]\nv = 2\nleft = [{ views = [\"files\", \"search\"], share = 1.0 }]\nright = [{ views = [\"outline\"], share = 0.5 }, { views = [\"live-feed.panel\"], share = 0.5 }]\nbottom = []\n\n[[layouts.items]]\nname = \"Revisão ampla\"\n\n[layouts.items.arrangement]\nv = 2\nleft = []\nright = []\nbottom = [{ views = [\"outline\", \"files\"], share = 1.0 }]\n";

#[test]
fn a_settings_file_without_panels_reads_none_and_gains_only_its_table() {
    let old = AppSettings::from_doc(&doc(SETTINGS_0_6_4));
    assert_eq!(old.panels, None);
    let text = applied(SETTINGS_0_6_4, &SettingChange::Panels(Some(docked())));
    assert_eq!(
        text,
        format!("{SETTINGS_0_6_4}\r\n{}", DOCKED_TOML.replace('\n', "\r\n"))
    );
    let read = AppSettings::from_doc(&doc(&text));
    assert_eq!(read.panels, Some(docked()));
    assert_eq!(read.theme, ThemePref::Dark);
    assert_eq!(read.right_views, None);
    // back to the default: the table goes, the file is as it was
    assert_eq!(applied(&text, &SettingChange::Panels(None)), SETTINGS_0_6_4);
    assert_eq!(
        applied(SETTINGS_0_6_4, &SettingChange::Panels(None)),
        SETTINGS_0_6_4
    );
}

#[test]
fn changed_panels_keep_their_place_and_the_rest_of_the_file() {
    let text = "theme = \"light\"\n\n# where the views sit\n[panels]\nv = 2\nleft = [{ views = [\"files\"], share = 1 }]\nright = []\nbottom = []\n\n[keymap]\n\"tab.close\" = \"Mod+W\"\n";
    // a whole number share (hand-written) reads as a fraction
    assert_eq!(
        AppSettings::from_doc(&doc(text)).panels,
        Some(Panels {
            left: vec![view_group(&["files"], 1.0)],
            ..no_panels()
        })
    );
    assert_eq!(
        applied(text, &SettingChange::Panels(Some(docked()))),
        format!("theme = \"light\"\n\n# where the views sit\n{DOCKED_TOML}\n[keymap]\n\"tab.close\" = \"Mod+W\"\n")
    );
}

#[test]
fn unreadable_panels_read_as_none_and_the_rest_survives() {
    for bad in [
        "[panels]\nleft = []\nright = []\nbottom = []\n",
        "[panels]\nv = -1\nleft = []\nright = []\nbottom = []\n",
        "[panels]\nv = \"2\"\nleft = []\nright = []\nbottom = []\n",
        "[panels]\nv = 2\n",
        "[panels]\nv = 2\nleft = []\nright = []\n",
        "[panels]\nv = 2\nleft = \"files\"\nright = []\nbottom = []\n",
        "[panels]\nv = 2\nleft = []\nright = [1, 2]\nbottom = []\n",
        "[panels]\nv = 2\nleft = [{ views = \"files\", share = 0.5 }]\nright = []\nbottom = []\n",
        "[panels]\nv = 2\nleft = []\nright = [{ views = [1], share = 0.5 }]\nbottom = []\n",
        "[panels]\nv = 2\nleft = []\nright = []\nbottom = [{ views = [\"outline\"] }]\n",
        "[panels]\nv = 2\nleft = [{ views = [\"files\"], share = \"half\" }]\nright = []\nbottom = []\n",
        "[panels]\nv = 2\nleft = []\nright = [{ views = [\"outline\"], share = nan }]\nbottom = []\n",
        "[panels]\nv = 2\nleft = []\nright = []\nbottom = [{ views = [\"outline\"], share = inf }]\n",
        "panels = 3\n",
    ] {
        // the keys around it read on: the old arrangement of the right column too
        let text = format!("theme = \"dark\"\n{bad}\n[right_views]\nv = 1\ngroups = []\n");
        let read = AppSettings::from_doc(&doc(&text));
        assert_eq!(read.panels, None, "{bad}");
        assert_eq!(read.theme, ThemePref::Dark, "{bad}");
        assert_eq!(
            read.right_views,
            Some(RightViews {
                v: 1,
                groups: Vec::new()
            }),
            "{bad}"
        );
    }
    // the other spelling of an array of tables reads too; an unknown key inside is ignored
    let text = "[panels]\nv = 3\nmine = true\nright = []\nbottom = []\n\n[[panels.left]]\nviews = [\"files\", \"x:y\"]\nshare = 0.25\n\n[[panels.left]]\nviews = []\nshare = 0.75\n";
    assert_eq!(
        AppSettings::from_doc(&doc(text)).panels,
        Some(Panels {
            v: 3,
            left: vec![view_group(&["files", "x:y"], 0.25), view_group(&[], 0.75)],
            ..no_panels()
        })
    );
    assert_eq!(
        AppSettings::from_doc(&doc(
            "panels = { v = 2, left = [], right = [], bottom = [] }\n"
        ))
        .panels,
        Some(no_panels())
    );
}

#[test]
fn panels_out_of_bounds_are_refused_before_they_reach_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    fs::write(file.path(), "theme = \"dark\"\n").unwrap();
    let long = "x".repeat(65);
    let many: Vec<&str> = vec!["a"; 17];
    for bad in [
        vec![view_group(&["a"], 0.25); 4],
        vec![view_group(&many, 1.0)],
        vec![view_group(&[""], 1.0)],
        vec![view_group(&[long.as_str()], 1.0)],
        vec![view_group(&["a b"], 1.0)],
        vec![view_group(&["ü"], 1.0)],
        vec![view_group(&["a"], 1.5)],
        vec![view_group(&["a"], -0.1)],
        vec![view_group(&["a"], f64::NAN)],
        vec![view_group(&["a"], f64::INFINITY)],
    ] {
        // one rule for every region
        for panels in [
            Panels {
                left: bad.clone(),
                ..docked()
            },
            Panels {
                right: bad.clone(),
                ..docked()
            },
            Panels {
                bottom: bad.clone(),
                ..docked()
            },
        ] {
            let change = SettingChange::Panels(Some(panels));
            assert!(
                matches!(file.apply(&change), Err(SettingsError::Invalid { ref reason, .. }) if reason == "panels"),
                "{change:?}"
            );
        }
    }
    assert_eq!(
        fs::read_to_string(file.path()).unwrap(),
        "theme = \"dark\"\n"
    );
    // in bounds: the most groups in every region at once; an empty arrangement is a value too
    let full = Panels {
        v: 2,
        left: vec![view_group(&["a"], 0.3); 3],
        right: vec![view_group(&["b"], 0.3); 3],
        bottom: vec![view_group(&["c"], 0.3); 3],
    };
    let saved = file
        .apply(&SettingChange::Panels(Some(full.clone())))
        .unwrap();
    assert_eq!(saved.panels, Some(full.clone()));
    assert_eq!(file.load().panels, Some(full));
    file.apply(&SettingChange::Panels(Some(no_panels())))
        .unwrap();
    assert_eq!(file.load().panels, Some(no_panels()));
    assert_eq!(file.load().theme, ThemePref::Dark);
}

#[test]
fn the_panels_change_wire_format() {
    let json = r#"{"key":"panels","value":{"v":2,"left":[{"views":["files","search"],"share":1.0}],"right":[{"views":["outline"],"share":0.5},{"views":["live-feed.panel"],"share":0.5}],"bottom":[]}}"#;
    let change: SettingChange = serde_json::from_str(json).unwrap();
    assert_eq!(change, SettingChange::Panels(Some(docked())));
    assert_eq!(change.key(), "panels");
    assert_eq!(serde_json::to_string(&change).unwrap(), json);
    let reset: SettingChange = serde_json::from_str(r#"{"key":"panels","value":null}"#).unwrap();
    assert_eq!(reset, SettingChange::Panels(None));
    // no value at all = the reset (serde reads an absent `Option` content as `None`)
    assert_eq!(
        serde_json::from_str::<SettingChange>(r#"{"key":"panels"}"#).unwrap(),
        SettingChange::Panels(None)
    );
    for bad in [
        r#"{"key":"panels","value":{"left":[],"right":[],"bottom":[]}}"#,
        r#"{"key":"panels","value":{"v":2,"left":[],"right":[]}}"#,
        r#"{"key":"panels","value":{"v":2,"left":[{"views":"files","share":1}],"right":[],"bottom":[]}}"#,
    ] {
        assert!(serde_json::from_str::<SettingChange>(bad).is_err(), "{bad}");
    }
    let settings = serde_json::to_value(AppSettings::default()).unwrap();
    assert_eq!(settings["panels"], serde_json::Value::Null);
    let settings = serde_json::to_value(AppSettings {
        panels: Some(docked()),
        ..AppSettings::default()
    })
    .unwrap();
    assert_eq!(settings["panels"]["left"][0]["views"][1], "search");
    assert_eq!(settings["panels"]["v"], 2);
}

#[test]
fn the_right_column_and_the_panels_are_written_apart() {
    let right = "[right_views]\nv = 1\ngroups = [{ views = [\"outline\"], share = 1.0 }]\n";
    let text = format!("# mine\n{right}\n{DOCKED_TOML}");
    let read = AppSettings::from_doc(&doc(&text));
    let outline = RightViews {
        v: 1,
        groups: vec![view_group(&["outline"], 1.0)],
    };
    assert_eq!(read.right_views, Some(outline.clone()));
    assert_eq!(read.panels, Some(docked()));
    // a change of one leaves the bytes of the other
    assert_eq!(
        applied(&text, &SettingChange::Panels(Some(no_panels()))),
        format!("# mine\n{right}\n[panels]\nv = 2\nleft = []\nright = []\nbottom = []\n")
    );
    assert_eq!(
        applied(&text, &SettingChange::RightViews(Some(stacked()))),
        format!("# mine\n[right_views]\nv = 1\ngroups = [{{ views = [\"outline\"], share = 0.5 }}, {{ views = [\"live-feed.panel\"], share = 0.5 }}]\n\n{DOCKED_TOML}")
    );
    // and each goes on its own
    let alone = AppSettings::from_doc(&doc(&applied(&text, &SettingChange::Panels(None))));
    assert_eq!((alone.right_views, alone.panels), (Some(outline), None));
    let alone = AppSettings::from_doc(&doc(&applied(&text, &SettingChange::RightViews(None))));
    assert_eq!((alone.right_views, alone.panels), (None, Some(docked())));
}

#[test]
fn a_settings_file_without_layouts_reads_none_and_gains_only_its_table() {
    let old = AppSettings::from_doc(&doc(SETTINGS_0_6_4));
    assert_eq!(old.layouts, None);
    let text = applied(SETTINGS_0_6_4, &SettingChange::Layouts(Some(named())));
    assert_eq!(
        text,
        format!("{SETTINGS_0_6_4}\r\n{}", NAMED_TOML.replace('\n', "\r\n"))
    );
    let read = AppSettings::from_doc(&doc(&text));
    assert_eq!(read.layouts, Some(named()));
    assert_eq!(read.theme, ThemePref::Dark);
    assert_eq!(read.panels, None);
    // no layout left to keep: the table goes with everything under it, the file is as it was
    assert_eq!(
        applied(&text, &SettingChange::Layouts(None)),
        SETTINGS_0_6_4
    );
    assert_eq!(
        applied(SETTINGS_0_6_4, &SettingChange::Layouts(None)),
        SETTINGS_0_6_4
    );
}

#[test]
fn changed_layouts_keep_their_place_and_the_rest_of_the_file() {
    let around = |layouts: &str| {
        format!("theme = \"light\"\n\n# my layouts\n{layouts}\n{DOCKED_TOML}\n[keymap]\n\"tab.close\" = \"Mod+W\"\n")
    };
    let text = around("[layouts]\nv = 1\n\n[[layouts.items]]\nname = \"Old\"\n\n[layouts.items.arrangement]\nv = 2\nleft = []\nright = []\nbottom = []\n");
    assert_eq!(
        AppSettings::from_doc(&doc(&text)).layouts,
        Some(Layouts {
            v: 1,
            items: vec![LayoutItem {
                name: "Old".to_owned(),
                arrangement: no_panels(),
            }],
        })
    );
    // written whole: the layouts of before are gone, the tables after it stay after it
    let out = applied(&text, &SettingChange::Layouts(Some(named())));
    assert_eq!(out, around(NAMED_TOML));
    let read = AppSettings::from_doc(&doc(&out));
    assert_eq!(read.layouts, Some(named()));
    assert_eq!(read.panels, Some(docked()));
    // fewer layouts, then none: an empty list is written as one
    let mut one = named();
    one.items.truncate(1);
    let out = applied(&out, &SettingChange::Layouts(Some(one.clone())));
    assert!(!out.contains("Revisão"), "{out}");
    assert_eq!(AppSettings::from_doc(&doc(&out)).layouts, Some(one));
    let empty = Layouts {
        v: 1,
        items: Vec::new(),
    };
    let out = applied(&out, &SettingChange::Layouts(Some(empty.clone())));
    assert_eq!(out, around("[layouts]\nv = 1\nitems = []\n"));
    assert_eq!(AppSettings::from_doc(&doc(&out)).layouts, Some(empty));
    // and filled again from the empty list
    assert_eq!(
        applied(&out, &SettingChange::Layouts(Some(named()))),
        around(NAMED_TOML)
    );
}

#[test]
fn unreadable_layouts_read_as_none_and_the_rest_survives() {
    const EMPTY: &str = "{ v = 2, left = [], right = [], bottom = [] }";
    for bad in [
        "[layouts]\nitems = []\n".to_owned(),
        "[layouts]\nv = -1\nitems = []\n".to_owned(),
        "[layouts]\nv = \"1\"\nitems = []\n".to_owned(),
        "[layouts]\nv = 1\n".to_owned(),
        "[layouts]\nv = 1\nitems = \"Writing\"\n".to_owned(),
        "[layouts]\nv = 1\nitems = [1, 2]\n".to_owned(),
        format!("[layouts]\nv = 1\nitems = [{{ name = 5, arrangement = {EMPTY} }}]\n"),
        format!("[layouts]\nv = 1\nitems = [{{ arrangement = {EMPTY} }}]\n"),
        "[layouts]\nv = 1\nitems = [{ name = \"a\" }]\n".to_owned(),
        "[layouts]\nv = 1\nitems = [{ name = \"a\", arrangement = 3 }]\n".to_owned(),
        "[layouts]\nv = 1\nitems = [{ name = \"a\", arrangement = { left = [], right = [], bottom = [] } }]\n".to_owned(),
        "[layouts]\nv = 1\nitems = [{ name = \"a\", arrangement = { v = 2, left = [{ views = [\"x\"], share = \"half\" }], right = [], bottom = [] } }]\n".to_owned(),
        // one damaged layout damages the list
        "[layouts]\nv = 1\n\n[[layouts.items]]\nname = \"whole\"\n\n[layouts.items.arrangement]\nv = 2\nleft = []\nright = []\nbottom = []\n\n[[layouts.items]]\nname = \"damaged\"\n".to_owned(),
        "layouts = 3\n".to_owned(),
    ] {
        let text = format!("theme = \"dark\"\n{bad}\n{DOCKED_TOML}");
        let read = AppSettings::from_doc(&doc(&text));
        assert_eq!(read.layouts, None, "{bad}");
        assert_eq!(read.theme, ThemePref::Dark, "{bad}");
        assert_eq!(read.panels, Some(docked()), "{bad}");
    }
    // damaged panels leave the layouts readable
    let text = format!("[panels]\nv = 2\n\n{NAMED_TOML}");
    let read = AppSettings::from_doc(&doc(&text));
    assert_eq!((read.panels, read.layouts), (None, Some(named())));
    // inline tables read too; an unknown key inside is ignored
    let text = format!("[layouts]\nv = 4\nmine = true\nitems = [{{ name = \"a\", color = \"red\", arrangement = {EMPTY} }}]\n");
    assert_eq!(
        AppSettings::from_doc(&doc(&text)).layouts,
        Some(Layouts {
            v: 4,
            items: vec![LayoutItem {
                name: "a".to_owned(),
                arrangement: no_panels(),
            }],
        })
    );
}

#[test]
fn layouts_out_of_bounds_are_refused_before_they_reach_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    fs::write(file.path(), "theme = \"dark\"\n").unwrap();
    let item = |name: &str| LayoutItem {
        name: name.to_owned(),
        arrangement: docked(),
    };
    let holding = |arrangement: Panels| LayoutItem {
        name: "held".to_owned(),
        arrangement,
    };
    for items in [
        (0..21).map(|n| item(&format!("Layout {n}"))).collect(),
        vec![item("")],
        vec![item("   ")],
        vec![item(" Writing")],
        vec![item("Writing\u{a0}")],
        vec![item(&"x".repeat(41))],
        vec![item("two\nlines")],
        vec![item("a\tb")],
        vec![item("a\u{7f}b")],
        vec![item("a\u{9b}b")],
        // the arrangement of a layout follows the rule of `[panels]`
        vec![
            item("fine"),
            holding(Panels {
                bottom: vec![view_group(&["a"], 0.25); 4],
                ..docked()
            }),
        ],
        vec![holding(Panels {
            left: vec![view_group(&["a b"], 1.0)],
            ..docked()
        })],
        vec![holding(Panels {
            right: vec![view_group(&["a"], f64::NAN)],
            ..docked()
        })],
    ] {
        let change = SettingChange::Layouts(Some(Layouts { v: 1, items }));
        assert!(
            matches!(file.apply(&change), Err(SettingsError::Invalid { ref reason, .. }) if reason == "layouts"),
            "{change:?}"
        );
    }
    assert_eq!(
        fs::read_to_string(file.path()).unwrap(),
        "theme = \"dark\"\n"
    );
    // in bounds: the most layouts; the longest name counts characters, not bytes; spaces inside a name
    let mut items: Vec<LayoutItem> = (0..19).map(|n| item(&format!("Layout {n}"))).collect();
    items.push(item(&"ã".repeat(40)));
    let full = Layouts { v: 1, items };
    let saved = file
        .apply(&SettingChange::Layouts(Some(full.clone())))
        .unwrap();
    assert_eq!(saved.layouts, Some(full.clone()));
    assert_eq!(file.load().layouts, Some(full));
    assert_eq!(file.load().theme, ThemePref::Dark);
}

#[test]
fn the_layouts_change_wire_format() {
    let json = r#"{"key":"layouts","value":{"v":1,"items":[{"name":"Writing","arrangement":{"v":2,"left":[{"views":["files","search"],"share":1.0}],"right":[{"views":["outline"],"share":0.5},{"views":["live-feed.panel"],"share":0.5}],"bottom":[]}}]}}"#;
    let mut one = named();
    one.items.truncate(1);
    let change: SettingChange = serde_json::from_str(json).unwrap();
    assert_eq!(change, SettingChange::Layouts(Some(one.clone())));
    assert_eq!(change.key(), "layouts");
    assert_eq!(serde_json::to_string(&change).unwrap(), json);
    let reset: SettingChange = serde_json::from_str(r#"{"key":"layouts","value":null}"#).unwrap();
    assert_eq!(reset, SettingChange::Layouts(None));
    assert_eq!(
        serde_json::from_str::<SettingChange>(r#"{"key":"layouts"}"#).unwrap(),
        SettingChange::Layouts(None)
    );
    for bad in [
        r#"{"key":"layouts","value":{"items":[]}}"#,
        r#"{"key":"layouts","value":{"v":1}}"#,
        r#"{"key":"layouts","value":{"v":1,"items":[{"name":"a"}]}}"#,
        r#"{"key":"layouts","value":{"v":1,"items":[{"name":5,"arrangement":{"v":2,"left":[],"right":[],"bottom":[]}}]}}"#,
    ] {
        assert!(serde_json::from_str::<SettingChange>(bad).is_err(), "{bad}");
    }
    let settings = serde_json::to_value(AppSettings::default()).unwrap();
    assert_eq!(settings["layouts"], serde_json::Value::Null);
    let settings = serde_json::to_value(AppSettings {
        layouts: Some(one),
        ..AppSettings::default()
    })
    .unwrap();
    assert_eq!(settings["layouts"]["items"][0]["name"], "Writing");
    assert_eq!(
        settings["layouts"]["items"][0]["arrangement"]["right"][1]["views"][0],
        "live-feed.panel"
    );
}

/// The two changes of a key that holds a format number: a value in this build's format, and the removal.
fn changes_of(key: &str) -> [SettingChange; 2] {
    if key == "panels" {
        [
            SettingChange::Panels(Some(docked())),
            SettingChange::Panels(None),
        ]
    } else {
        [
            SettingChange::Layouts(Some(named())),
            SettingChange::Layouts(None),
        ]
    }
}

#[test]
fn a_table_of_a_newer_format_is_never_written_over_or_removed() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    for (key, table) in [
        // the shape of today, then shapes this build cannot read
        (
            "panels",
            "[panels]\nv = 99\nleft = []\nright = []\nbottom = []\n",
        ),
        ("panels", "[panels]\nv = 3\ntop = { views = [\"files\"] }\n"),
        ("panels", "panels = { v = 3, docks = 4 }\n"),
        ("layouts", "[layouts]\nv = 99\nitems = []\n"),
        (
            "layouts",
            "[layouts]\nv = 2\n\n[layouts.saved.Writing]\nleft = [\"files\"]\n",
        ),
    ] {
        let stored = format!("theme = \"dark\" # night\n# from a newer PerMark\n{table}\n[keymap]\n\"tab.close\" = \"Mod+W\"\n");
        fs::write(file.path(), &stored).unwrap();
        for change in changes_of(key) {
            assert!(
                matches!(
                    file.apply(&change),
                    Err(SettingsError::Invalid { key: ref at, ref reason }) if at == key && reason == key
                ),
                "{change:?} over {table}"
            );
            assert_eq!(
                fs::read_to_string(file.path()).unwrap(),
                stored,
                "{change:?} over {table}"
            );
            // the document itself is refused before anything in it changes
            let mut held = doc(&stored);
            assert!(apply_change(&mut held, &change).is_err(), "{change:?}");
            assert_eq!(held.to_string(), stored, "{change:?} over {table}");
        }
        // the keys around it are written as ever, and the newer table stays as it is
        let saved = file.apply(&SettingChange::Tone(Tone::Paper)).unwrap();
        assert_eq!((saved.theme, saved.tone), (ThemePref::Dark, Tone::Paper));
        let text = fs::read_to_string(file.path()).unwrap();
        assert!(
            text.contains(&format!("# from a newer PerMark\n{table}")),
            "{text}"
        );
        assert!(
            text.ends_with("\n[keymap]\n\"tab.close\" = \"Mod+W\"\n"),
            "{text}"
        );
    }
}

#[test]
fn a_table_of_this_format_of_an_older_one_or_without_one_is_written_over() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    for (key, stored) in [
        (
            "panels",
            "[panels]\nv = 2\nleft = []\nright = []\nbottom = []\n",
        ),
        (
            "panels",
            "[panels]\nv = 1\nleft = []\nright = []\nbottom = []\n",
        ),
        (
            "panels",
            "[panels]\nv = \"x\"\nleft = []\nright = []\nbottom = []\n",
        ),
        ("panels", "[panels]\nleft = []\nright = []\nbottom = []\n"),
        ("panels", "[panels]\nv = -99\n"),
        ("panels", "[panels]\nv = 99.5\n"),
        ("panels", "panels = 99\n"),
        ("layouts", "[layouts]\nv = 1\nitems = []\n"),
        ("layouts", "[layouts]\nv = 0\nitems = []\n"),
        ("layouts", "[layouts]\nv = \"x\"\nitems = []\n"),
        ("layouts", "[layouts]\nitems = []\n"),
        ("layouts", "layouts = 99\n"),
    ] {
        let [write, remove] = changes_of(key);
        let written = if key == "panels" {
            (Some(docked()), None)
        } else {
            (None, Some(named()))
        };
        fs::write(file.path(), stored).unwrap();
        let saved = file.apply(&write).unwrap();
        assert_eq!((saved.panels, saved.layouts), written, "{stored}");
        let read = file.load();
        assert_eq!((read.panels, read.layouts), written, "{stored}");
        fs::write(file.path(), stored).unwrap();
        file.apply(&remove).unwrap();
        assert!(
            !fs::read_to_string(file.path()).unwrap().contains(key),
            "{stored}"
        );
    }
    // a removal counts as this build's format; a value that claims the stored newer format is no way around it
    let ahead = "[panels]\nv = 3\nleft = []\nright = []\nbottom = []\n";
    fs::write(file.path(), ahead).unwrap();
    for change in [
        SettingChange::Panels(Some(docked())),
        SettingChange::Panels(None),
        SettingChange::Panels(Some(Panels { v: 3, ..docked() })),
    ] {
        assert!(file.apply(&change).is_err(), "{change:?}");
        assert_eq!(fs::read_to_string(file.path()).unwrap(), ahead);
    }
    // the right column's own key keeps its rule: written whole, whatever is stored
    fs::write(file.path(), "[right_views]\nv = 99\ngroups = []\n").unwrap();
    assert_eq!(
        file.apply(&SettingChange::RightViews(Some(stacked())))
            .unwrap()
            .right_views,
        Some(stacked())
    );
    file.apply(&SettingChange::RightViews(None)).unwrap();
    assert_eq!(file.load().right_views, None);
}

/// A value in another format would be stored but not understood here, and a higher one would shut every later
/// write of its key out: only the formats this build writes reach the file.
#[test]
fn a_value_in_a_format_this_build_does_not_write_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    assert_eq!(
        (docked().v, named().v),
        (PANELS_VERSION, LAYOUTS_VERSION),
        "the fixtures are in the formats written"
    );
    let stored = format!("theme = \"dark\"\n\n{DOCKED_TOML}\n{NAMED_TOML}");
    fs::write(file.path(), &stored).unwrap();
    let holding = |v: u32| {
        let mut layouts = named();
        layouts.items[1].arrangement.v = v;
        layouts
    };
    for (word, change) in [
        (
            "panels",
            SettingChange::Panels(Some(Panels { v: 99, ..docked() })),
        ),
        (
            "panels",
            SettingChange::Panels(Some(Panels { v: 1, ..docked() })),
        ),
        (
            "panels",
            SettingChange::Panels(Some(Panels {
                v: 0,
                ..no_panels()
            })),
        ),
        (
            "layouts",
            SettingChange::Layouts(Some(Layouts { v: 99, ..named() })),
        ),
        (
            "layouts",
            SettingChange::Layouts(Some(Layouts {
                v: 0,
                items: Vec::new(),
            })),
        ),
        // one layout whose arrangement is in another format
        ("layouts", SettingChange::Layouts(Some(holding(99)))),
        ("layouts", SettingChange::Layouts(Some(holding(1)))),
    ] {
        assert!(
            matches!(
                file.apply(&change),
                Err(SettingsError::Invalid { ref key, ref reason }) if key == word && reason == word
            ),
            "{change:?}"
        );
        assert_eq!(
            fs::read_to_string(file.path()).unwrap(),
            stored,
            "{change:?}"
        );
    }
    // the formats written still go in, over what is stored
    let saved = file
        .apply(&SettingChange::Panels(Some(no_panels())))
        .unwrap();
    assert_eq!(saved.panels, Some(no_panels()));
    let saved = file
        .apply(&SettingChange::Layouts(Some(holding(PANELS_VERSION))))
        .unwrap();
    assert_eq!(saved.layouts, Some(named()));
    let read = file.load();
    assert_eq!(
        (read.panels, read.layouts, read.theme),
        (Some(no_panels()), Some(named()), ThemePref::Dark)
    );
    // the right column's own key carries any format number, as before
    let other = RightViews { v: 7, ..stacked() };
    assert_eq!(
        file.apply(&SettingChange::RightViews(Some(other.clone())))
            .unwrap()
            .right_views,
        Some(other)
    );
}

// ---- characters file (issue #38: emoji + symbol pickers) -------------------------------------------------

/// A written groups file, both table spellings, a name that needs trimming.
const GROUPS_MINE: &str = "[[groups]]\nname = \"mine\"\n[[groups.entries]]\nchar = \"★\"\nnames = [\"star\", \" ★ \"]\n\n[[groups]]\nname = \"inline\"\nentries = [{ char = \"→\", names = [\"right arrow\"] }]\n";

#[test]
fn the_groups_file_parses_from_the_profile_with_the_built_ins_as_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("profile");
    fs::create_dir_all(&profile).unwrap();
    // empty name | missing file = the built-ins
    assert_eq!(
        characters::load(&profile, "").unwrap(),
        characters::defaults()
    );
    assert_eq!(
        characters::load(&profile, "characters.toml").unwrap(),
        characters::defaults()
    );
    // a file that is not TOML: the typed error, never a silent default
    fs::write(profile.join("bad.toml"), "name = [[[").unwrap();
    assert!(matches!(
        characters::load(&profile, "bad.toml"),
        Err(CharactersError::Unreadable { .. })
    ));
    // a written file: both spellings parse, names are trimmed
    fs::write(profile.join("chars.toml"), GROUPS_MINE).unwrap();
    let groups = characters::load(&profile, "chars.toml").unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name, "mine");
    assert_eq!(groups[0].entries.len(), 1);
    assert_eq!(groups[0].entries[0].char, "★");
    assert_eq!(groups[0].entries[0].names, ["star", "★"]);
    assert_eq!(groups[1].name, "inline");
    assert_eq!(groups[1].entries[0].char, "→");
    // the built-ins are groups of the same shape
    let built = characters::defaults();
    assert!(built.iter().all(|group| {
        !group.name.is_empty()
            && !group.entries.is_empty()
            && group.entries.iter().all(|entry| !entry.char.is_empty())
    }));
    assert!(built.iter().any(|group| group.name == "greek"));
    assert!(built.iter().any(|group| group.name == "emoji"));
}

#[test]
fn characters_file_is_validated_written_and_read_back() {
    for bad in ["a/b", "..", "chars.json", "con.toml"] {
        assert!(
            SettingChange::CharactersFile(bad.to_owned())
                .validate()
                .is_err(),
            "{bad}"
        );
    }
    assert!(SettingChange::CharactersFile(String::new())
        .validate()
        .is_ok());
    assert!(SettingChange::CharactersFile("  ".to_owned())
        .validate()
        .is_ok());
    assert!(SettingChange::CharactersFile("my chars.TOML".to_owned())
        .validate()
        .is_ok());
    assert_eq!(
        SettingChange::CharactersFile("chars.toml".to_owned()).key(),
        "characters_file"
    );
    // through the file: the named groups and their path ride on the settings
    let dir = tempfile::tempdir().unwrap();
    let file = settings_file(dir.path());
    fs::write(dir.path().join("profile").join("chars.toml"), GROUPS_MINE).unwrap();
    let s = file
        .apply(&SettingChange::CharactersFile("chars.toml".to_owned()))
        .unwrap();
    assert_eq!(s.characters_file, "chars.toml");
    assert_eq!(s.characters.len(), 2);
    assert_eq!(s.characters[0].entries[0].char, "★");
    assert_eq!(
        s.characters_path,
        dir.path()
            .join("profile")
            .join("chars.toml")
            .display()
            .to_string()
    );
    let again = file.load();
    assert_eq!(again.characters, s.characters);
    assert_eq!(again.characters_path, s.characters_path);
    // empty = the built-ins only, and the empty stays in the file
    let cleared = file
        .apply(&SettingChange::CharactersFile(String::new()))
        .unwrap();
    assert_eq!(cleared.characters_file, "");
    assert_eq!(cleared.characters, characters::defaults());
    assert_eq!(cleared.characters_path, "");
    let text = fs::read_to_string(file.path()).unwrap();
    assert!(text.contains("characters_file = \"\""), "{text:?}");
}
