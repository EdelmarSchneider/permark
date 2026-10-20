//! The packaging files name each other: the Store manifest, the shell extension, the packaging script, the Store overlay,
//! the platform overlays and the installer. Nothing here installs anything; these tests keep them from drifting apart.

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
