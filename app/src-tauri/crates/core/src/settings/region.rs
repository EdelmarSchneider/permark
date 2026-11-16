//! The system's format region (how dates and numbers are written), for a window to format them like the rest of the
//! system when it speaks the same language. Windows keeps the "Regional format" apart from the display language and the
//! web view only reports the latter, so the app asks the system here.

/// The format region as a language tag (`en-IN`): Windows' "Regional format"; on Linux the locale of dates (`LC_ALL`,
/// `LC_TIME`, `LANG`); `None` on macOS (the web view's languages carry the region there) and when the system names
/// none (`C`, `POSIX`).
#[must_use]
pub fn system_region() -> Option<String> {
    os_region().and_then(|name| region_tag(&name))
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
