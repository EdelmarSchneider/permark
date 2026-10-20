//! Windows: who opens a file type now. Read only: Windows protects the choice, PerMark never writes it.

use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
use winreg::RegKey;

use super::FileGroup;

/// Where Explorer records the person's choice per extension.
const FILE_EXTS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

/// The `ProgID` the person chose for `extension` (`.md`): newer Windows records it in `UserChoiceLatest\ProgId`.
#[must_use]
pub fn user_choice(extension: &str) -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let progid_in = |path: String| {
        hkcu.open_subkey(path)
            .ok()?
            .get_value::<String, _>("ProgId")
            .ok()
            .filter(|p| !p.is_empty())
    };
    let base = format!(r"{FILE_EXTS}\{extension}");
    progid_in(format!(r"{base}\UserChoiceLatest\ProgId"))
        .or_else(|| progid_in(format!(r"{base}\UserChoice")))
}

/// The package app a packaged `ProgID` (`AppX…`) belongs to (`HKCR\<progid>\Application\AppUserModelID`).
fn app_user_model_id(prog_id: &str) -> Option<String> {
    RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(format!(r"{prog_id}\Application"))
        .ok()?
        .get_value::<String, _>("AppUserModelID")
        .ok()
}

/// A `ProgID` is PerMark's: the installer's (`PerMark.*`), or one of PerMark's Store package.
#[must_use]
pub fn is_ours(prog_id: &str, package_app: impl Fn(&str) -> Option<String>) -> bool {
    let lower = prog_id.to_ascii_lowercase();
    if lower.starts_with("permark.") {
        return true;
    }
    lower.starts_with("appx")
        && package_app(prog_id).is_some_and(|aumid| {
            let name = aumid.split('_').next().unwrap_or_default();
            name.eq_ignore_ascii_case("EdelmarSchneider.TrailMark")
                || name.eq_ignore_ascii_case("EdelmarSchneider.PerMarkSideload")
        })
}

/// PerMark opens the group's main extension (`.md` for Markdown) now: the others follow the person's own pick.
#[must_use]
pub fn group_is_ours(group: FileGroup) -> bool {
    group.extensions().first().is_some_and(|extension| {
        user_choice(extension).is_some_and(|prog_id| is_ours(&prog_id, app_user_model_id))
    })
}
