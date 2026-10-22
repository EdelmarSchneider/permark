//! The language the Windows installer recorded: read once, at the first start (`SettingsFile::adopt_language`).

use super::LanguagePref;

/// What the installer wrote in `HKEY_CURRENT_USER\Software\PerMark\Language` (Windows); nothing elsewhere.
#[must_use]
pub fn installer_language() -> Option<LanguagePref> {
    installer_code().and_then(|code| language_from_code(&code))
}

/// A recorded code as a language: one of the thirteen (spaces and line breaks around it ignored); `system` and anything
/// else are not a choice.
#[must_use]
pub fn language_from_code(code: &str) -> Option<LanguagePref> {
    match LanguagePref::parse(code.trim()) {
        Some(LanguagePref::System) | None => None,
        found => found,
    }
}

#[cfg(windows)]
fn installer_code() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\PerMark")
        .ok()?
        .get_value::<String, _>("Language")
        .ok()
}

#[cfg(not(windows))]
fn installer_code() -> Option<String> {
    None
}
