//! The two system calls of the plugin: telling Explorer that associations changed, and asking the display languages.

// Plain FFI calls into the Windows shell; each unsafe block says why it is sound.
#![allow(unsafe_code)]

use std::ptr;

use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

/// Explorer redraws its menus and icons. May take a moment while Explorer is busy: callers run it off the UI thread.
pub fn associations_changed() {
    // the event id is a flag that fits an i32 (0x0800_0000)
    let event = i32::try_from(SHCNE_ASSOCCHANGED).unwrap_or(i32::MAX);
    // SAFETY: `SHCNE_ASSOCCHANGED` takes no item pointers; null is what the API documents for them.
    unsafe {
        SHChangeNotify(event, SHCNF_IDLIST, ptr::null(), ptr::null());
    }
}

/// The person's display languages, best first (`de-DE`); empty when Windows does not say.
#[must_use]
pub fn display_languages() -> Vec<String> {
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
