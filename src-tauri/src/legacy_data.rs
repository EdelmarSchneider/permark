//! Carries the app's folders over from the identifier the app had before it was renamed.
//!
//! The identifier names the folders where the system keeps the app's config, data and local data (on Windows the local one
//! also holds the web view's profile, which is where the window's own settings live). A new identifier means new, empty
//! folders, so the first start of the renamed app copies each old folder to its new place. It copies and does not move:
//! the old folder stays as a backup, and an older version of the app that is still installed keeps working.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The identifier the app had before.
const LEGACY_IDENTIFIER: &str = "dev.trailmark.app";

/// An old folder, and the new place it is copied to.
#[derive(Debug, PartialEq, Eq)]
struct Carry {
    from: PathBuf,
    to: PathBuf,
}

/// The folders to copy for the app named `identifier`, given the system's base folders (config, data, local data). A base
/// that appears twice (on Windows, config and data are the same) is copied once. The old folder is the base with the old
/// identifier; folders that are already there under the new one are not touched.
fn plan(identifier: &str, bases: &[PathBuf]) -> Vec<Carry> {
    let mut out: Vec<Carry> = Vec::new();
    for base in bases {
        let carry = Carry {
            from: base.join(LEGACY_IDENTIFIER),
            to: base.join(identifier),
        };
        if carry.from != carry.to && !out.contains(&carry) {
            out.push(carry);
        }
    }
    out.retain(|carry| carry.from.is_dir() && !carry.to.exists());
    out
}

/// Copies the old folders of the app named `identifier` to their new places. Best effort: what cannot be copied is reported
/// and skipped, and the app starts anyway.
pub(crate) fn carry_over(identifier: &str, bases: &[PathBuf]) {
    for carry in plan(identifier, bases) {
        // copied under another name and renamed at the end, so a folder cut short is never taken for the finished one
        let partial = carry.to.with_extension("migrating");
        let _ = fs::remove_dir_all(&partial);
        copy_tree(&carry.from, &partial);
        if let Err(error) = fs::rename(&partial, &carry.to) {
            eprintln!(
                "cannot move {} to {}: {error}",
                partial.display(),
                carry.to.display()
            );
        }
    }
}

/// [`carry_over`] for the system's config, data and local data folders.
pub(crate) fn carry_over_system(identifier: &str) {
    let bases: Vec<PathBuf> = [dirs::config_dir(), dirs::data_dir(), dirs::data_local_dir()]
        .into_iter()
        .flatten()
        .collect();
    carry_over(identifier, &bases);
}

/// Copies a folder and everything in it, skipping (and reporting) what cannot be read, such as a file another program has open.
fn copy_tree(from: &Path, to: &Path) {
    if let Err(error) = fs::create_dir_all(to) {
        eprintln!("cannot make {}: {error}", to.display());
        return;
    }
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) => return report(from, &error),
    };
    for entry in entries.flatten() {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => copy_tree(&source, &target),
            Ok(kind) if kind.is_file() => {
                if let Err(error) = fs::copy(&source, &target) {
                    report(&source, &error);
                }
            }
            // links are not followed: they could lead anywhere
            _ => {}
        }
    }
}

fn report(path: &Path, error: &io::Error) {
    eprintln!("cannot copy {}: {error}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_with_old(dir: &Path, name: &str) -> PathBuf {
        let base = dir.join(name);
        let old = base.join(LEGACY_IDENTIFIER);
        fs::create_dir_all(old.join("EBWebView").join("Default")).expect("mkdir");
        fs::write(old.join("settings.json"), "{\"a\":1}").expect("write");
        fs::write(old.join("EBWebView").join("Default").join("x"), "x").expect("write");
        base
    }

    #[test]
    fn the_old_folder_is_copied_and_kept() {
        let dir = tempfile::tempdir().expect("tmp");
        let base = base_with_old(dir.path(), "roaming");

        carry_over("dev.permark.app", std::slice::from_ref(&base));

        let new = base.join("dev.permark.app");
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).expect("read"),
            "{\"a\":1}"
        );
        assert!(new.join("EBWebView").join("Default").join("x").is_file());
        assert!(base.join(LEGACY_IDENTIFIER).join("settings.json").is_file());
        assert!(!base.join("dev.permark.migrating").exists());
    }

    #[test]
    fn a_new_folder_that_exists_is_left_alone() {
        let dir = tempfile::tempdir().expect("tmp");
        let base = base_with_old(dir.path(), "roaming");
        fs::create_dir_all(base.join("dev.permark.app")).expect("mkdir");

        carry_over("dev.permark.app", std::slice::from_ref(&base));

        assert!(!base.join("dev.permark.app").join("settings.json").exists());
    }

    #[test]
    fn a_base_listed_twice_is_planned_once_and_a_missing_old_folder_is_skipped() {
        let dir = tempfile::tempdir().expect("tmp");
        let one = base_with_old(dir.path(), "roaming");
        let none = dir.path().join("empty");
        fs::create_dir_all(&none).expect("mkdir");

        let planned = plan("dev.permark.app", &[one.clone(), one.clone(), none]);

        assert_eq!(
            planned,
            vec![Carry {
                from: one.join(LEGACY_IDENTIFIER),
                to: one.join("dev.permark.app"),
            }]
        );
    }

    #[test]
    fn the_old_identifier_itself_plans_nothing() {
        let dir = tempfile::tempdir().expect("tmp");
        let base = base_with_old(dir.path(), "roaming");

        assert_eq!(plan(LEGACY_IDENTIFIER, &[base]).len(), 0);
    }
}
