//! App-wide settings: the ones that belong to the person, not to a project (today, the interface language).
//!
//! They live in AppData as `settings.json`. Parsing is forgiving: a value this build does not understand
//! (written by a newer version) reads as "not chosen" and the keys it does not know are kept on the next
//! write, so a downgrade and an upgrade never lose each other's settings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize};

use super::SessionError;
use crate::util::write_atomic;

const FILE_NAME: &str = "settings.json";

/// The interface language. The UI owns the translations; the app only keeps the choice, plus whatever the
/// Windows installer recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    /// Follow the language of the operating system.
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "de")]
    De,
    #[serde(rename = "zh-CN")]
    ZhCn,
}

impl Language {
    /// The language an installer wrote down (`en`, `pt-BR`, `es`, `de`, `zh-CN`). "system" is not an installer
    /// choice, and a code from a newer installer reads as nothing.
    pub fn from_installer_code(code: &str) -> Option<Self> {
        match code.trim() {
            "en" => Some(Self::En),
            "pt-BR" => Some(Self::PtBr),
            "es" => Some(Self::Es),
            "de" => Some(Self::De),
            "zh-CN" => Some(Self::ZhCn),
            _ => None,
        }
    }
}

/// Where TrailMark opens when it is started with no folder or file to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StartIn {
    /// The welcome screen, with the recent folders and the latest notes.
    #[default]
    #[serde(rename = "welcome")]
    Welcome,
    /// The Notes window.
    #[serde(rename = "notes")]
    Notes,
}

/// Reads an `Option<T>`, or `None` when the stored value is not one this build understands.
fn lenient_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Lenient<T> {
        Known(Option<T>),
        Unknown(IgnoredAny),
    }
    Ok(match Lenient::<T>::deserialize(deserializer)? {
        Lenient::Known(value) => value,
        Lenient::Unknown(_) => None,
    })
}

/// What `settings.json` holds. A missing key means "the person has not chosen yet".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// The language picked in the app. `None` until the person picks one (even "System default" counts as a
    /// pick), so the installer's choice can seed the first run without ever overriding a later one.
    #[serde(
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub language: Option<Language>,
    /// The command that opens a file in the person's editor (`code -g {path}:{line}:{col}`). `None` or blank: the
    /// system's default application. It lives here, not with the project, so a folder can never choose what is run.
    #[serde(
        rename = "editorCommand",
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub editor_command: Option<String>,
    /// The folder the person chose for their notes. `None`: the default folder (see `notes::folder`).
    #[serde(
        rename = "notesFolder",
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub notes_folder: Option<String>,
    /// Where TrailMark opens when it is started with nothing to open. `None`: the welcome screen.
    #[serde(
        rename = "startIn",
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub start_in: Option<StartIn>,
    /// Keys written by other versions, kept as they were.
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// The language the UI starts in: the person's own pick wins; before any pick, the installer's choice;
/// before that, the system's.
pub fn effective_language(stored: Option<Language>, installer: Option<Language>) -> Language {
    stored.or(installer).unwrap_or_default()
}

pub struct AppSettingsStore {
    file: PathBuf,
}

impl AppSettingsStore {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            file: app_data_dir.join(FILE_NAME),
        }
    }

    /// The saved settings (the defaults when the file is missing or unreadable).
    pub fn load(&self) -> AppSettings {
        std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Records the language the person picked, keeping every other setting.
    pub fn set_language(&self, language: Language) -> Result<(), SessionError> {
        let mut settings = self.load();
        settings.language = Some(language);
        self.save(&settings)
    }

    /// Records the editor command (blank: the system's default application), keeping every other setting.
    pub fn set_editor_command(&self, command: &str) -> Result<(), SessionError> {
        let mut settings = self.load();
        settings.editor_command = Some(command.trim().to_owned());
        self.save(&settings)
    }

    /// Records the folder the person chose for their notes, keeping every other setting.
    pub fn set_notes_folder(&self, folder: &str) -> Result<(), SessionError> {
        let mut settings = self.load();
        settings.notes_folder = Some(folder.trim().to_owned());
        self.save(&settings)
    }

    /// Records where TrailMark opens when it has nothing to open, keeping every other setting.
    pub fn set_start_in(&self, start_in: StartIn) -> Result<(), SessionError> {
        let mut settings = self.load();
        settings.start_in = Some(start_in);
        self.save(&settings)
    }

    fn save(&self, settings: &AppSettings) -> Result<(), SessionError> {
        let bytes =
            serde_json::to_vec_pretty(settings).map_err(|e| SessionError::Json(e.to_string()))?;
        write_atomic(&self.file, &bytes).map_err(|e| SessionError::io(&self.file, e))
    }
}

/// What the Windows installer recorded in `HKCU\Software\TrailMark\Language`; nothing elsewhere.
pub fn installer_language() -> Option<Language> {
    installer_language_code().and_then(|code| Language::from_installer_code(&code))
}

#[cfg(windows)]
fn installer_language_code() -> Option<String> {
    language_value_under("Software\\TrailMark")
}

/// The `Language` value of a key under `HKEY_CURRENT_USER`, when there is one.
#[cfg(windows)]
fn language_value_under(subkey: &str) -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(subkey)
        .ok()?
        .get_value::<String, _>("Language")
        .ok()
}

#[cfg(not(windows))]
fn installer_language_code() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, AppSettingsStore) {
        let d = tempfile::tempdir().expect("tempdir");
        let s = AppSettingsStore::new(d.path());
        (d, s)
    }

    #[test]
    fn nothing_is_chosen_until_the_person_picks() {
        let (_d, s) = store();
        assert_eq!(s.load().language, None);
        assert_eq!(s.load(), AppSettings::default());
    }

    #[test]
    fn every_language_round_trips_under_its_ui_code() {
        for (language, code) in [
            (Language::System, "system"),
            (Language::En, "en"),
            (Language::PtBr, "pt-BR"),
            (Language::Es, "es"),
            (Language::De, "de"),
            (Language::ZhCn, "zh-CN"),
        ] {
            let (_d, s) = store();
            s.set_language(language).expect("save");
            assert_eq!(s.load().language, Some(language));
            let text = std::fs::read_to_string(s.file.clone()).expect("read");
            let json: serde_json::Value = serde_json::from_str(&text).expect("json");
            assert_eq!(json["language"], code);
            assert_eq!(serde_json::to_value(language).expect("json"), code);
        }
    }

    #[test]
    fn choosing_system_default_is_a_choice() {
        let (_d, s) = store();
        s.set_language(Language::System).expect("save");
        let settings = s.load();
        assert_eq!(settings.language, Some(Language::System));
        // so an installer's language no longer applies
        assert_eq!(
            effective_language(settings.language, Some(Language::De)),
            Language::System
        );
    }

    #[test]
    fn a_language_from_a_newer_version_reads_as_not_chosen_and_nothing_else_is_lost() {
        for value in ["\"fr\"", "\"PT-br\"", "7", "true", "[\"de\"]", "{}", "null"] {
            let (_d, s) = store();
            std::fs::write(
                &s.file,
                format!("{{\"language\": {value}, \"theme\": \"sepia\"}}"),
            )
            .expect("write");
            assert_eq!(s.load().language, None, "{value}");
            s.set_language(Language::De).expect("save");
            let json: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&s.file).expect("read")).expect("json");
            assert_eq!(json["language"], "de");
            assert_eq!(json["theme"], "sepia", "unknown keys survive a write");
        }
    }

    #[test]
    fn a_corrupt_file_reads_as_defaults_and_is_replaced_on_write() {
        let (_d, s) = store();
        std::fs::write(&s.file, "{{{ not json").expect("write");
        assert_eq!(s.load(), AppSettings::default());
        s.set_language(Language::Es).expect("save");
        assert_eq!(s.load().language, Some(Language::Es));
    }

    #[test]
    fn the_editor_command_is_kept_trimmed_and_does_not_disturb_the_language() {
        let (_d, s) = store();
        assert_eq!(s.load().editor_command, None);
        s.set_language(Language::De).expect("save");
        s.set_editor_command("  code -g {path}:{line}:{col} \n")
            .expect("save");
        let settings = s.load();
        assert_eq!(
            settings.editor_command.as_deref(),
            Some("code -g {path}:{line}:{col}")
        );
        assert_eq!(settings.language, Some(Language::De));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&s.file).expect("read")).expect("json");
        assert_eq!(json["editorCommand"], "code -g {path}:{line}:{col}");
        // a blank command is a choice too: the system's default application
        s.set_editor_command("").expect("save");
        assert_eq!(s.load().editor_command.as_deref(), Some(""));
        s.set_language(Language::En).expect("save");
        assert_eq!(s.load().editor_command.as_deref(), Some(""));
    }

    #[test]
    fn an_editor_command_this_build_does_not_understand_reads_as_not_chosen() {
        for value in ["7", "true", "[\"code\"]", "{}", "null"] {
            let (_d, s) = store();
            std::fs::write(&s.file, format!("{{\"editorCommand\": {value}}}")).expect("write");
            assert_eq!(s.load().editor_command, None, "{value}");
        }
    }

    #[test]
    fn the_notes_folder_and_the_start_screen_are_kept_and_disturb_nothing_else() {
        let (_d, s) = store();
        let settings = s.load();
        assert_eq!((settings.notes_folder, settings.start_in), (None, None));
        s.set_language(Language::De).expect("save");
        s.set_notes_folder("  /sync/notes ").expect("save");
        s.set_start_in(StartIn::Notes).expect("save");
        let settings = s.load();
        assert_eq!(settings.notes_folder.as_deref(), Some("/sync/notes"));
        assert_eq!(settings.start_in, Some(StartIn::Notes));
        assert_eq!(settings.language, Some(Language::De));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&s.file).expect("read")).expect("json");
        assert_eq!(json["notesFolder"], "/sync/notes");
        assert_eq!(json["startIn"], "notes");
        s.set_start_in(StartIn::Welcome).expect("save");
        assert_eq!(s.load().start_in, Some(StartIn::Welcome));
        assert_eq!(s.load().notes_folder.as_deref(), Some("/sync/notes"));
    }

    #[test]
    fn a_notes_setting_this_build_does_not_understand_reads_as_not_chosen() {
        for value in ["7", "true", "[\"x\"]", "{}", "null"] {
            let (_d, s) = store();
            std::fs::write(
                &s.file,
                format!("{{\"notesFolder\": {value}, \"startIn\": {value}}}"),
            )
            .expect("write");
            let settings = s.load();
            assert_eq!(settings.notes_folder, None, "{value}");
            assert_eq!(settings.start_in, None, "{value}");
        }
        let (_d, s) = store();
        std::fs::write(&s.file, "{\"startIn\": \"somewhere\"}").expect("write");
        assert_eq!(s.load().start_in, None);
    }

    #[test]
    fn the_persons_pick_beats_the_installer_which_beats_the_system() {
        use Language::{De, En, PtBr, System};
        assert_eq!(effective_language(None, None), System);
        assert_eq!(effective_language(None, Some(PtBr)), PtBr);
        assert_eq!(effective_language(Some(En), Some(PtBr)), En);
        assert_eq!(effective_language(Some(De), None), De);
        assert_eq!(effective_language(Some(System), Some(PtBr)), System);
    }

    #[test]
    fn installer_codes_map_to_languages_and_unknown_ones_to_nothing() {
        assert_eq!(Language::from_installer_code("en"), Some(Language::En));
        assert_eq!(Language::from_installer_code("pt-BR"), Some(Language::PtBr));
        assert_eq!(Language::from_installer_code("es"), Some(Language::Es));
        assert_eq!(Language::from_installer_code(" de\r\n"), Some(Language::De));
        assert_eq!(Language::from_installer_code("zh-CN"), Some(Language::ZhCn));
        for code in [
            "", "system", "fr", "pt", "pt-br", "zh", "zh-cn", "zh-TW", "2052", "1046",
        ] {
            assert_eq!(Language::from_installer_code(code), None, "{code}");
        }
    }

    /// The installer hook (`installer/hooks.nsh`) writes the codes this module reads: every code it can write is
    /// one the app understands, for each of the five languages, and the config turns the language dialog on.
    #[test]
    fn the_installer_writes_exactly_the_codes_the_app_reads() {
        const HOOKS: &str = include_str!("../../installer/hooks.nsh");
        const CONFIG: &str = include_str!("../../tauri.conf.json");

        let written: Vec<&str> = HOOKS
            .lines()
            .filter_map(|line| line.trim().strip_prefix("StrCpy $R8 \""))
            .filter_map(|rest| rest.strip_suffix('"'))
            .filter(|code| !code.is_empty())
            .collect();
        assert_eq!(written, ["pt-BR", "es", "de", "zh-CN", "en"]);
        for code in &written {
            assert!(Language::from_installer_code(code).is_some(), "{code}");
        }
        // the registry location the app reads (the define is `Software\TrailMark`; the tests of the installer script
        // point it elsewhere to try the script out)
        assert!(HOOKS.contains(r#"!define TRAILMARK_APP_KEY "Software\TrailMark""#));
        assert!(HOOKS.contains(r#"WriteRegStr HKCU "${TRAILMARK_APP_KEY}" "Language" "$R8""#));

        let config: serde_json::Value = serde_json::from_str(CONFIG).expect("tauri.conf.json");
        let nsis = &config["bundle"]["windows"]["nsis"];
        assert_eq!(nsis["displayLanguageSelector"], true);
        assert_eq!(nsis["installerHooks"], "installer/hooks.nsh");
        let languages: Vec<&str> = nsis["languages"]
            .as_array()
            .expect("languages")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        // English first: it is the fallback when the system's language is not one of the five
        assert_eq!(
            languages,
            [
                "English",
                "PortugueseBR",
                "SpanishInternational",
                "German",
                "SimpChinese"
            ]
        );
    }

    /// The reader finds the value the installer writes (checked on a throwaway key, not the real one).
    #[cfg(windows)]
    #[test]
    fn the_language_value_is_read_from_the_registry() {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let subkey = format!("Software\\TrailMarkTest{}", std::process::id());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        assert_eq!(language_value_under(&subkey), None, "no key, no value");
        let (key, _) = hkcu.create_subkey(&subkey).expect("create the key");
        assert_eq!(
            language_value_under(&subkey),
            None,
            "a key without the value"
        );
        key.set_value("Language", &"pt-BR")
            .expect("write the value");
        let read = language_value_under(&subkey);
        hkcu.delete_subkey_all(&subkey).expect("clean up");
        assert_eq!(read.as_deref(), Some("pt-BR"));
        assert_eq!(
            read.and_then(|code| Language::from_installer_code(&code)),
            Some(Language::PtBr)
        );
    }
}
