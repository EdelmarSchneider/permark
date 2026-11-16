//! The packaging files name each other: the Store manifest, the shell extension, the packaging script, the Store overlay,
//! the platform overlays and the installer. Nothing here installs anything; these tests keep them from drifting apart.

use std::collections::BTreeMap;

use permark_core::desktop::FileGroup;

const MANIFEST: &str = include_str!("../msstore/AppxManifest.xml");
const SHELL_CRATE: &str = include_str!("../crates/shell-ext/Cargo.toml");
const SHELL_SOURCE: &str = include_str!("../crates/shell-ext/src/lib.rs");
const SCRIPT: &str = include_str!("../../../scripts/build-msix.ps1");
const STORE_OVERLAY: &str = include_str!("../tauri.msstore.conf.json");
const CONFIG: &str = include_str!("../tauri.conf.json");
const MACOS: &str = include_str!("../tauri.macos.conf.json");
const LINUX: &str = include_str!("../tauri.linux.conf.json");
const DESKTOP: &str = include_str!("../linux/permark.desktop");
const WORKSPACE: &str = include_str!("../Cargo.toml");
const HOOKS: &str = include_str!("../installer/hooks.nsh");
/// Windows language ids of `bundle.windows.nsis.languages`, in the same order.
const INSTALLER_LANGUAGE_IDS: [u32; 18] = [
    1033, 1046, 3082, 1031, 2052, 1040, 1041, 1042, 1036, 2070, 1027, 1110, 1048, 1028, 1057, 1066,
    1049, 1055,
];
/// Installer languages Tauri does not translate (`bundle.windows.nsis.customLanguageFiles`).
const CUSTOM_LANGUAGES: [(&str, &str); 4] = [
    (
        "Catalan",
        include_str!("../installer/languages/Catalan.nsh"),
    ),
    (
        "Galician",
        include_str!("../installer/languages/Galician.nsh"),
    ),
    (
        "Romanian",
        include_str!("../installer/languages/Romanian.nsh"),
    ),
    (
        "Indonesian",
        include_str!("../installer/languages/Indonesian.nsh"),
    ),
];

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("valid JSON")
}

#[test]
fn the_package_declares_the_alias_and_the_program() {
    let program = format!("{}.exe", env!("CARGO_PKG_NAME"));
    assert!(MANIFEST.contains(&format!(r#"Alias="{program}""#)));
    assert!(MANIFEST.contains(&format!(r#"Executable="{program}""#)));
    // a file association hands the path over as the first argument, as `permark <path>` takes it
    assert!(MANIFEST.contains(r#"Parameters="&quot;%1&quot;""#));
    assert!(MANIFEST.contains(r#"Version="@VERSION@""#));
    assert!(MANIFEST.contains(r#"Description="@DESCRIPTION@""#));
    // the identity Partner Center reserved
    assert!(MANIFEST.contains(r#"Name="EdelmarSchneider.TrailMark""#));
}

#[test]
fn the_package_offers_every_file_group() {
    for group in FileGroup::ALL {
        for ext in group.extensions() {
            assert!(
                MANIFEST.contains(&format!("<uap:FileType>{ext}</uap:FileType>")),
                "{ext}"
            );
        }
    }
}

#[test]
fn the_folder_menu_entries_name_the_dll_and_its_classes() {
    assert!(SHELL_CRATE.contains(r#"name = "permark_shell""#));
    assert!(SHELL_CRATE.contains(r#"crate-type = ["cdylib""#));
    assert!(MANIFEST.contains(r#"Path="permark_shell.dll""#));
    assert!(SCRIPT.contains("permark_shell.dll"));
    for (text, number) in [
        (
            "D5FFB935-7A42-4E3D-90E1-0180A7999B94",
            "0xD5FF_B935_7A42_4E3D_90E1_0180_A799_9B94",
        ),
        (
            "C3B7E2A1-9D54-4C18-B6F0-2E8A71D45C09",
            "0xC3B7_E2A1_9D54_4C18_B6F0_2E8A_71D4_5C09",
        ),
    ] {
        assert!(MANIFEST.contains(&format!(r#"<com:Class Id="{text}""#)));
        // on folders and on the background of a folder
        assert_eq!(MANIFEST.matches(&format!(r#"Clsid="{text}""#)).count(), 2);
        assert!(SHELL_SOURCE.contains(number));
    }
    assert!(MANIFEST.contains(r#"<desktop5:ItemType Type="Directory">"#));
    assert!(MANIFEST.contains(r#"<desktop5:ItemType Type="Directory\Background">"#));
    // loaded by a helper process, never by Explorer
    assert!(MANIFEST.contains("<com:SurrogateServer"));
}

#[test]
fn the_script_builds_the_store_flavor_and_the_unwinding_dll() {
    assert!(SCRIPT.contains("--no-default-features --features store"));
    assert!(SCRIPT.contains("--profile release-shell -p permark-shell"));
    assert!(SCRIPT.contains(r"target\release-shell\permark_shell.dll"));
    assert!(WORKSPACE.contains("[profile.release-shell]"));
    let profile = &WORKSPACE[WORKSPACE.find("[profile.release-shell]").unwrap_or(0)..];
    assert!(profile.contains(r#"panic = "unwind""#));
    // the store build has no updater and no registry writer
    assert!(WORKSPACE.contains(r#"store = ["common"]"#));
    let common_start = WORKSPACE.find("\ncommon = [").expect("common feature");
    // one line (a Windows checkout may end it with CRLF)
    let common = &WORKSPACE[common_start..]
        .split_once(']')
        .map_or("", |(list, _)| list);
    assert!(!common.contains("plugin-updates") && !common.contains("plugin-shell-integration"));
}

#[test]
fn the_store_overlay_removes_the_updater() {
    let overlay = json(STORE_OVERLAY);
    assert!(overlay["plugins"]["updater"].is_null());
    assert!(overlay["plugins"]
        .as_object()
        .is_some_and(|plugins| plugins.contains_key("updater")));
    assert_eq!(overlay["bundle"]["createUpdaterArtifacts"], false);
}

#[test]
fn windows_registers_through_the_hooks_never_through_tauri_associations() {
    let config = json(CONFIG);
    // Tauri's NSIS association writes the extension's default value (a takeover): only the platform overlays have them
    assert!(config["bundle"]["fileAssociations"].is_null());
    assert_eq!(
        config["bundle"]["windows"]["nsis"]["installerHooks"],
        "installer/hooks.nsh"
    );
    assert_eq!(config["bundle"]["createUpdaterArtifacts"], true);
    assert!(config["plugins"]["updater"]["pubkey"]
        .as_str()
        .is_some_and(|k| !k.is_empty()));
    assert_eq!(
        config["plugins"]["updater"]["endpoints"][0],
        "https://github.com/EdelmarSchneider/permark/releases/latest/download/latest.json"
    );
    // the windows are made in code (with the web view data folder in PerMark's profile)
    assert_eq!(config["app"]["windows"], serde_json::json!([]));
}

#[test]
fn macos_and_linux_list_every_group_and_never_claim_it() {
    for overlay in [MACOS, LINUX] {
        let overlay = json(overlay);
        let associations = overlay["bundle"]["fileAssociations"]
            .as_array()
            .expect("associations")
            .clone();
        assert_eq!(associations.len(), FileGroup::ALL.len());
        for (group, association) in FileGroup::ALL.iter().zip(&associations) {
            assert_eq!(association["rank"], "Alternate", "{group:?}");
            let exts: Vec<String> = group
                .extensions()
                .iter()
                .map(|e| e.trim_start_matches('.').to_owned())
                .collect();
            assert_eq!(association["ext"], serde_json::json!(exts));
        }
    }
    let linux = json(LINUX);
    for (group, association) in FileGroup::ALL.iter().zip(
        linux["bundle"]["fileAssociations"]
            .as_array()
            .into_iter()
            .flatten(),
    ) {
        let mime = association["mimeType"].as_str().expect("a MIME type");
        assert!(group.mime_types().contains(&mime), "{group:?}");
    }
    // the desktop entry passes the file over
    assert!(DESKTOP.contains("Exec={{exec}} %f"));
    assert!(DESKTOP.contains("MimeType={{mime_type}};"));
}

#[test]
fn the_third_party_notices_ship_in_every_installer_and_in_the_store_package() {
    // a placeholder is tracked so that every build has the file; the release jobs replace it
    // (`npm run notices`, app/scripts/third-party-notices.mjs)
    let placeholder = include_str!("../resources/THIRD-PARTY-NOTICES.md");
    assert!(placeholder.starts_with("# Third-party notices"));
    let config = json(CONFIG);
    assert_eq!(
        config["bundle"]["resources"]["resources/THIRD-PARTY-NOTICES.md"],
        "THIRD-PARTY-NOTICES.md"
    );
    // the Store package is laid out by hand: the script copies the file beside the program
    assert!(SCRIPT.contains("THIRD-PARTY-NOTICES.md"));
    // the app crate is not third-party software (cargo-about skips crates that are never published)
    assert!(WORKSPACE.contains("publish = false"));
}

#[test]
fn the_custom_installer_languages_are_listed_and_carry_both_spellings_of_the_install_choice() {
    let config = json(CONFIG);
    let listed = config["bundle"]["windows"]["nsis"]["customLanguageFiles"]
        .as_object()
        .expect("customLanguageFiles");
    assert_eq!(listed.len(), CUSTOM_LANGUAGES.len());
    for (name, text) in CUSTOM_LANGUAGES {
        assert_eq!(listed[name], format!("installer/languages/{name}.nsh"));
        // the text of `LangString <key> ${LANG_...} "<text>"`
        let value = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(&format!("LangString {key} ")))
                .and_then(|rest| rest.split_once(' '))
                .map(|(_, text)| text.to_owned())
        };
        // Tauri CLI 2.12.4 reads `chooseHowToInstall`, 2.12.3 the misspelled `choowHowToInstall`: both, same words
        let new = value("chooseHowToInstall");
        assert!(new.is_some(), "{name}.nsh lacks chooseHowToInstall");
        assert_eq!(new, value("choowHowToInstall"), "{name}.nsh");
    }
}

#[test]
fn every_text_of_the_installer_hooks_is_in_every_installer_language() {
    let config = json(CONFIG);
    let languages = config["bundle"]["windows"]["nsis"]["languages"]
        .as_array()
        .map_or(0, Vec::len);
    assert_eq!(languages, INSTALLER_LANGUAGE_IDS.len());
    // `LangString <key> <language id> "<text>"`
    let mut texts: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for line in HOOKS.lines() {
        let Some(rest) = line.strip_prefix("LangString ") else {
            continue;
        };
        let mut parts = rest.splitn(3, ' ');
        let (Some(key), Some(id)) = (parts.next(), parts.next()) else {
            continue;
        };
        let id = id.parse().unwrap_or(0);
        texts.entry(key).or_default().push(id);
    }
    // the PATH option and what it tells the person when it leaves the PATH alone
    for key in ["PathOption", "PathNotAdded", "PathNotRemoved"] {
        assert!(texts.contains_key(key), "hooks.nsh lacks {key}");
    }
    let mut expected = INSTALLER_LANGUAGE_IDS.to_vec();
    expected.sort_unstable();
    for (key, mut ids) in texts {
        ids.sort_unstable();
        assert_eq!(ids, expected, "{key}");
    }
}

#[test]
fn the_path_option_is_off_by_default_per_user_and_never_cuts_a_long_path() {
    // the user's PATH only: nothing machine-wide, no administrator rights
    assert!(HOOKS.contains(r#"!define PERMARK_ENV_KEY "Environment""#));
    assert!(!HOOKS.contains("HKLM"));
    // off unless an earlier install recorded it on: on the welcome page and in a silent install
    assert!(HOOKS.contains(r#""${PERMARK_PATH_CHOICE}" 0"#));
    assert!(HOOKS.contains(r#"PERMARK_RESOLVE $PermarkPath "${PERMARK_PATH_CHOICE}" "0""#));
    // the size is read before the text, and the new value has to fit an NSIS string
    assert!(HOOKS.contains("RegQueryValueExW"));
    assert!(HOOKS.contains("${NSIS_MAX_STRLEN}"));
    // the PATH is written in one place only, with the type it had
    let writes: Vec<&str> = HOOKS
        .lines()
        .map(str::trim_start)
        .filter(|line| line.contains(r#""${PERMARK_ENV_KEY}" "Path""#) && line.starts_with("Write"))
        .collect();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert!(writes.iter().any(|line| line.starts_with("WriteRegStr ")));
    assert!(writes
        .iter()
        .any(|line| line.starts_with("WriteRegExpandStr ")));
    // terminals started afterwards see the change
    assert!(HOOKS.contains(
        r#"SendMessageTimeoutW(p ${HWND_BROADCAST}, i ${WM_SETTINGCHANGE}, p 0, w "Environment""#
    ));
}
