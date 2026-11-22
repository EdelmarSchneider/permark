//! The system's languages and format region (how dates and numbers are written). Windows keeps the "Regional format"
//! apart from the display language and the web view only reports the latter, so the app asks the system here; the
//! languages also word what no page may word (the program-start question).

/// The format region as a language tag (`en-IN`): Windows' "Regional format"; on Linux the locale of dates (`LC_ALL`,
/// `LC_TIME`, `LANG`); `None` on macOS (the web view's languages carry the region there) and when the system names
/// none (`C`, `POSIX`).
#[must_use]
pub fn system_region() -> Option<String> {
    os_region().and_then(|name| region_tag(&name))
}

/// The person's languages, best first (`pt-BR`): Windows' display languages; on Linux the `LANGUAGE` list, then the
/// first locale of `LC_ALL`, `LC_MESSAGES`, `LANG`; empty on macOS (the web view's languages carry them there) and
/// when the system names none.
#[must_use]
pub fn system_languages() -> Vec<String> {
    os_languages()
}

/// The languages the environment names, as tags: the `:` list of `LANGUAGE` first (gettext's order of preference), then
/// the locale of messages; `C`, `POSIX` and names that are no tag are left out.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
pub(super) fn env_languages(var: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let listed = var("LANGUAGE").unwrap_or_default();
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| var(name).filter(|value| !value.is_empty()))
        .unwrap_or_default();
    listed
        .split(':')
        .chain([locale.as_str()])
        .filter_map(region_tag)
        .collect()
}

/// An OS locale name as a language tag: `en_IN.UTF-8` -> `en-IN`, `sr_RS@latin` -> `sr-RS`, Windows' `de-DE_phoneb`
/// (a sort order) -> `de-DE`; `None` for `C`, `POSIX` and anything that is not letters | digits between dashes.
pub(super) fn region_tag(name: &str) -> Option<String> {
    let name = name.split(['.', '@']).next()?.trim();
    let name = if name.contains('-') {
        name.split('_').next()?
    } else {
        name
    };
    let tag = name.replace('_', "-");
    let parts_ok = tag
        .split('-')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()));
    (parts_ok && tag != "C" && tag != "POSIX").then_some(tag)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn os_region() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    // LOCALE_NAME_MAX_LENGTH, the closing NUL included
    let mut buffer = [0u16; 85];
    let capacity = i32::try_from(buffer.len()).ok()?;
    // SAFETY: the buffer is valid for `capacity` UTF-16 units; the call writes at most that many, NUL included, and
    // returns that count (0 on failure).
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), capacity) };
    let length = usize::try_from(written).ok()?.checked_sub(1)?;
    buffer
        .get(..length)
        .map(String::from_utf16_lossy)
        .filter(|name| !name.is_empty())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_region() -> Option<String> {
    ["LC_ALL", "LC_TIME", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn os_region() -> Option<String> {
    None
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn os_languages() -> Vec<String> {
    use std::ptr;
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0u32;
    let mut length = 0u32;
    // SAFETY: the first call only asks for the length (no buffer); the second fills a buffer of exactly that length. The
    // list is NUL-separated tags closed by an empty one.
    unsafe {
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &raw mut count,
            ptr::null_mut(),
            &raw mut length,
        ) == 0
            || length == 0
        {
            return Vec::new();
        }
        let mut buffer = vec![0u16; length as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &raw mut count,
            buffer.as_mut_ptr(),
            &raw mut length,
        ) == 0
        {
            return Vec::new();
        }
        buffer
            .split(|unit| *unit == 0)
            .filter(|tag| !tag.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_languages() -> Vec<String> {
    env_languages(|name| std::env::var(name).ok())
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn os_languages() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{env_languages, region_tag, system_languages};

    /// A fake environment of `pairs`.
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn linux_languages_read_the_language_list_then_the_message_locale() {
        assert_eq!(env_languages(env(&[("LANG", "pt_BR.UTF-8")])), ["pt-BR"]);
        let all = [
            ("LANGUAGE", "de_AT:fr"),
            ("LC_ALL", ""),
            ("LC_MESSAGES", "es_ES.UTF-8"),
            ("LANG", "pt_BR.UTF-8"),
        ];
        assert_eq!(env_languages(env(&all)), ["de-AT", "fr", "es-ES"]);
        assert_eq!(
            env_languages(env(&[("LC_ALL", "it_IT@euro"), ("LANG", "pt_BR")])),
            ["it-IT"]
        );
        // C | POSIX | nothing name no language; the dates' locale is the format region, not the language
        let none = Vec::<String>::new();
        assert_eq!(
            env_languages(env(&[("LANG", "C.UTF-8"), ("LANGUAGE", "")])),
            none
        );
        assert_eq!(env_languages(env(&[("LC_TIME", "en_IN.UTF-8")])), none);
        assert_eq!(env_languages(env(&[])), none);
    }

    #[test]
    fn the_systems_languages_are_tags() {
        // whatever the machine says, each is a tag
        for tag in system_languages() {
            assert!(region_tag(&tag).is_some(), "{tag}");
        }
    }
}
