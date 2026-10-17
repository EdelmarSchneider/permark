//! The other half of "Open with" on macOS and Linux is declared in the bundle configuration, which only the bundlers
//! read. These tests read the files themselves, so a mistake shows up on every platform without making a bundle.

use std::path::Path;

use serde_json::Value;
use tauri::utils::config::{BundleTypeRole, FileAssociation, HandlerRank, LinuxConfig};

const MAIN: &str = include_str!("../../tauri.conf.json");
const MACOS: &str = include_str!("../../tauri.macos.conf.json");
const LINUX: &str = include_str!("../../tauri.linux.conf.json");
const DESKTOP_TEMPLATE: &str = include_str!("../../installer/permark.desktop");

fn bundle(json: &str) -> Value {
    let config: Value = serde_json::from_str(json).expect("valid json");
    config["bundle"].clone()
}

/// Read as the bundler reads them: unknown fields and unknown role or rank names are errors.
fn associations(json: &str) -> Vec<FileAssociation> {
    serde_json::from_value(bundle(json)["fileAssociations"].clone())
        .expect("fileAssociations the bundler accepts")
}

fn exts(association: &FileAssociation) -> Vec<String> {
    association.ext.iter().map(ToString::to_string).collect()
}

#[test]
fn macos_lists_permark_for_markdown_files_without_taking_over_the_default() {
    let all = associations(MACOS);
    let markdown = all
        .iter()
        .find(|association| !association.ext.is_empty())
        .expect("a document type for Markdown files");

    assert_eq!(exts(markdown), ["md", "markdown"]);
    assert_eq!(markdown.role, BundleTypeRole::Viewer);
    assert_eq!(markdown.rank, HandlerRank::Alternate);
    assert_eq!(markdown.mime_type.as_deref(), Some("text/markdown"));
    assert_eq!(
        markdown.content_types.as_deref(),
        Some(&["net.daringfireball.markdown".to_owned()][..])
    );
    // The Markdown type belongs to others: it is referred to, never exported.
    assert_eq!(markdown.exported_type, None);
}

#[test]
fn macos_accepts_a_folder_dropped_on_the_dock_icon() {
    let all = associations(MACOS);
    let folder = all
        .iter()
        .find(|association| association.ext.is_empty())
        .expect("a document type for folders");

    assert_eq!(
        folder.content_types.as_deref(),
        Some(&["public.folder".to_owned()][..])
    );
    assert_eq!(folder.role, BundleTypeRole::Viewer);
    assert_eq!(folder.rank, HandlerRank::Alternate);
    assert_eq!(folder.exported_type, None);
    assert_eq!(all.len(), 2, "Markdown files and folders, nothing else");
}

#[test]
fn linux_declares_markdown_and_no_folders() {
    let all = associations(LINUX);

    assert_eq!(all.len(), 1);
    assert_eq!(exts(&all[0]), ["md", "markdown"]);
    assert_eq!(all[0].mime_type.as_deref(), Some("text/markdown"));
    // `inode/directory` can make an app the default folder handler on some desktops.
    assert!(!LINUX.contains("inode/directory"));
    assert!(!DESKTOP_TEMPLATE.contains("inode/directory"));
}

#[test]
fn windows_keeps_its_own_registration_and_declares_no_associations() {
    assert!(bundle(MAIN).get("fileAssociations").is_none());
}

/// The default entry is `Exec=permark` with no field code, which tells the file manager not to pass the file.
#[test]
fn the_desktop_entry_of_the_deb_and_rpm_passes_the_file_to_the_program() {
    let linux: LinuxConfig =
        serde_json::from_value(bundle(LINUX)["linux"].clone()).expect("linux bundle config");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));

    for template in [&linux.deb.desktop_template, &linux.rpm.desktop_template] {
        let template = template.as_deref().expect("a custom desktop template");
        let resolved = manifest.join(template);
        assert!(resolved.is_file(), "{} exists", resolved.display());
        assert_eq!(
            std::fs::read_to_string(&resolved).expect("template"),
            DESKTOP_TEMPLATE
        );
    }

    let exec = DESKTOP_TEMPLATE
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .expect("an Exec line");
    // One file per run: with several selected, the file manager starts the program once for each.
    assert_eq!(exec, "{{exec}} %f");
    assert!(DESKTOP_TEMPLATE
        .lines()
        .any(|line| line == "MimeType={{mime_type}};"));
}

/// The bundler fills `categories`, `comment`, `exec`, `icon`, `name` and `mime_type`; any other name renders empty.
#[test]
fn the_desktop_template_uses_only_what_the_bundler_provides() {
    let known = ["categories", "comment", "exec", "icon", "name", "mime_type"];
    let mut used = 0;
    for tag in DESKTOP_TEMPLATE.split("{{").skip(1) {
        let (inside, _) = tag.split_once("}}").expect("a closed tag");
        let name = inside.trim_start_matches('#').trim();
        if name.starts_with('/') {
            continue;
        }
        let name = name.strip_prefix("if ").unwrap_or(name);
        assert!(known.contains(&name), "unknown template variable {name}");
        used += 1;
    }
    assert!(used > 0);
    assert_eq!(
        DESKTOP_TEMPLATE.matches("{{#if").count(),
        DESKTOP_TEMPLATE.matches("{{/if}}").count()
    );
}
