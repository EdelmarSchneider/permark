//! "Open with TrailMark" in the file manager.
//!
//! On Windows two optional groups of registry entries, both under the current user (no administrator rights):
//!
//! * a **context menu** entry on folders, on the background of a folder and on Markdown files;
//! * a **file association**: TrailMark is offered in the "Open with" list of `.md` and `.markdown` files. Windows
//!   protects the default app of a file type from programs, so becoming the default is the person's step in
//!   Windows Settings ([`DEFAULT_APPS_URI`] opens it).
//!
//! The Windows installer writes the same entries (`installer/hooks.nsh`) and both remember the person's choices
//! under `HKCU\Software\TrailMark`, so an update neither re-adds what was removed nor removes what was kept.
//! On macOS and Linux nothing is registered yet ([`ShellState::supported`] is false).

mod entries;
#[cfg(windows)]
mod notify;
#[cfg(windows)]
mod registry;

use std::io;

use serde::Serialize;

/// The Windows Settings page where the default app of a file type is chosen.
pub const DEFAULT_APPS_URI: &str = "ms-settings:defaultapps";

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("the file manager entries are only available on Windows")]
    Unsupported,
    // The registry only exists on Windows.
    #[cfg_attr(not(windows), allow(dead_code))]
    #[error("cannot change the file manager entries: {0}")]
    Registry(io::Error),
}

/// What is registered right now, read from the registry each time (the person or another program may have
/// changed it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
// Four independent facts that go to the UI as they are; a state machine would only hide them.
#[allow(clippy::struct_excessive_bools)]
pub struct ShellState {
    /// False where TrailMark cannot register anything (every platform but Windows, for now).
    pub supported: bool,
    /// At least one "Open with TrailMark" entry of the context menu is there.
    pub context_menu: bool,
    /// TrailMark is registered as a program that opens Markdown files, so it is in their "Open with" list.
    pub association: bool,
    /// Windows' own choice for `.md` files is TrailMark.
    pub is_default: bool,
}

/// What is registered now.
#[cfg(windows)]
pub fn state() -> ShellState {
    registry::Registry::current_user().state()
}

#[cfg(not(windows))]
pub fn state() -> ShellState {
    ShellState::default()
}

/// Adds or removes the context-menu entries, worded in `locale` (`en`, `pt-BR`, `es`, `de`).
#[cfg(windows)]
pub fn set_context_menu(enabled: bool, locale: &str) -> Result<ShellState, ShellError> {
    let registry = registry::Registry::current_user();
    let exe = current_exe()?;
    registry
        .set_context_menu(enabled, &exe, entries::language_for(locale))
        .map_err(ShellError::Registry)?;
    Ok(registry.state())
}

/// Adds or removes TrailMark from the "Open with" list of Markdown files, worded in `locale`.
#[cfg(windows)]
pub fn set_association(enabled: bool, locale: &str) -> Result<ShellState, ShellError> {
    let registry = registry::Registry::current_user();
    let exe = current_exe()?;
    registry
        .set_association(enabled, &exe, entries::language_for(locale))
        .map_err(ShellError::Registry)?;
    Ok(registry.state())
}

/// Rewords the entries that exist in `locale`, after the interface language changed.
#[cfg(windows)]
pub fn refresh_labels(locale: &str) -> Result<(), ShellError> {
    registry::Registry::current_user()
        .refresh_labels(entries::language_for(locale))
        .map_err(ShellError::Registry)
}

#[cfg(not(windows))]
pub fn set_context_menu(_enabled: bool, _locale: &str) -> Result<ShellState, ShellError> {
    Err(ShellError::Unsupported)
}

#[cfg(not(windows))]
pub fn set_association(_enabled: bool, _locale: &str) -> Result<ShellState, ShellError> {
    Err(ShellError::Unsupported)
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)] // Same signature as the Windows version.
pub fn refresh_labels(_locale: &str) -> Result<(), ShellError> {
    Ok(())
}

/// The program the entries start: this one.
#[cfg(windows)]
fn current_exe() -> Result<std::path::PathBuf, ShellError> {
    let exe = std::env::current_exe().map_err(ShellError::Registry)?;
    Ok(dunce::simplified(&exe).to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The installer's script writes the layout `entries` describes, in the same keys and with the same
    /// command lines, the same labels in the four languages, and the same names for the person's choices.
    #[test]
    fn the_installer_writes_what_the_app_writes() {
        const HOOKS: &str = include_str!("../../installer/hooks.nsh");

        // where the entries go
        assert!(HOOKS.contains(r#"!define TRAILMARK_CLASSES_KEY "Software\Classes""#));
        assert!(HOOKS.contains(r#"!define TRAILMARK_APP_KEY "Software\TrailMark""#));
        // every key of the menu, with the placeholder its command passes
        for target in &entries::MENU_TARGETS {
            let line = format!(
                "!insertmacro OPENWITH_MENU_${{OP}} \"{}\" \"{}\"",
                target.key(),
                target.placeholder
            );
            assert!(HOOKS.contains(&line), "{line}");
        }
        // the ProgID and where it is listed
        assert!(HOOKS.contains(&format!("!define OPENWITH_PROGID \"{}\"", entries::PROG_ID)));
        for ext in entries::EXTENSIONS {
            let line = format!(
                "!insertmacro OPENWITH_LIST_${{OP}} \"{}\"",
                entries::open_with_key(ext)
            );
            assert!(HOOKS.contains(&line), "{line}");
        }
        for key in entries::association_keys(std::path::Path::new("X"), "x") {
            // each key the app writes is named in the script, relative to the ProgID
            let relative = key
                .path
                .strip_prefix(entries::PROG_ID)
                .expect("under the ProgID");
            assert!(
                relative.is_empty() || HOOKS.contains(&format!("${{OPENWITH_PROGID}}{relative}\"")),
                "{relative}"
            );
        }
        // the one value that is not the default value of a key
        assert!(HOOKS.contains(r#"\shell\open" "FriendlyAppName" "TrailMark""#));
        assert!(HOOKS.contains(r#""Icon" "$\"$OpenWithExe$\",0""#));
        // the text, in each language: the NSIS language id is the Windows LCID
        for (lcid, language) in [
            (1033, crate::session::Language::En),
            (1046, crate::session::Language::PtBr),
            (3082, crate::session::Language::Es),
            (1031, crate::session::Language::De),
        ] {
            for (name, text) in [
                ("OpenWithMenuLabel", entries::menu_label(language)),
                ("OpenWithTypeName", entries::type_name(language)),
            ] {
                let line = format!("LangString {name} {lcid} \"{text}\"");
                assert!(HOOKS.contains(&line), "{line}");
            }
        }
        // the choices the app and the installer both read and write
        for (define, name) in [
            ("OPENWITH_MENU_CHOICE", entries::MENU_CHOICE),
            ("OPENWITH_ASSOC_CHOICE", entries::ASSOCIATION_CHOICE),
        ] {
            let line = format!("!define {define} \"{name}\"");
            assert!(HOOKS.contains(&line), "{line}");
        }
    }

    /// Turning an entry off leaves the keys above it empty; the installer deletes the same ones the app does.
    #[test]
    fn the_installer_tidies_the_keys_the_app_tidies() {
        const HOOKS: &str = include_str!("../../installer/hooks.nsh");

        // what the app looks at: the keys it empties (the "Open with" list, an extension's `shell`) and all above them
        let mut tidied: Vec<String> = Vec::new();
        let mut starts: Vec<String> = entries::MENU_TARGETS
            .iter()
            .map(entries::MenuTarget::key)
            .collect();
        for ext in entries::EXTENSIONS {
            let list = entries::open_with_key(ext);
            let shell = entries::extension_shell_key(ext);
            tidied.push(list.clone());
            tidied.push(shell.clone());
            starts.extend([list, shell]);
        }
        for start in &starts {
            tidied.extend(entries::tidy_parents(start).into_iter().map(str::to_owned));
        }

        let in_script: Vec<&str> = HOOKS
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("!insertmacro OPENWITH_PRUNE \"")?
                    .strip_suffix('"')
            })
            .collect();
        for key in &tidied {
            assert!(in_script.contains(&key.as_str()), "{key}");
        }
        for key in &in_script {
            assert!(tidied.iter().any(|k| k == key), "{key}");
        }
        // the classes of Windows itself are never deleted
        assert!(
            !in_script.contains(&"Directory") && !in_script.contains(&"SystemFileAssociations")
        );
    }

    /// The installer's commands and the app's are the same text, with the program's path spelled by the script.
    #[test]
    fn the_installer_quotes_the_program_and_the_argument_like_the_app_does() {
        const HOOKS: &str = include_str!("../../installer/hooks.nsh");
        let app = entries::command_line(std::path::Path::new("PROGRAM"), "ARG");
        assert_eq!(app, r#""PROGRAM" "ARG""#);
        // the script's version, `$\"$OpenWithExe$\" $\"${ARGUMENT}$\"`, is the same shape
        assert!(HOOKS.contains(r#""$\"$OpenWithExe$\" $\"${ARGUMENT}$\"""#));
        assert!(HOOKS.contains(r#""$\"$OpenWithExe$\" $\"%1$\"""#));
    }

    #[test]
    fn nothing_is_supported_or_enabled_by_default() {
        let state = ShellState::default();
        assert!(!state.supported && !state.context_menu && !state.association && !state.is_default);
    }

    #[test]
    fn the_state_goes_to_the_ui_in_camel_case() {
        let json = serde_json::to_value(ShellState {
            supported: true,
            context_menu: true,
            association: false,
            is_default: false,
        })
        .expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "supported": true,
                "contextMenu": true,
                "association": false,
                "isDefault": false
            })
        );
    }
}
