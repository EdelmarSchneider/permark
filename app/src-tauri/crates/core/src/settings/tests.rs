use super::*;
use crate::paths::Root;
use crate::state::DEFAULT_CONFIG_TOML;

fn doc(text: &str) -> DocumentMut {
    text.parse().unwrap()
}

fn applied(text: &str, change: &SettingChange) -> String {
    let mut d = doc(text);
    apply_change(&mut d, change);
    render(&d, text)
}

#[test]
fn defaults_when_empty() {
    let s = AppSettings::from_doc(&doc(""));
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
    assert_eq!(file.load(), AppSettings::default());
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
fn a_corrupt_settings_file_is_kept_aside_not_overwritten() {
    let base = tempfile::tempdir().unwrap();
    let file = SettingsFile::app(&AppDirs::under(base.path()));
    std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    std::fs::write(file.path(), "theme = \"dark\n[[[").unwrap();
    assert_eq!(file.load(), AppSettings::default());
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
