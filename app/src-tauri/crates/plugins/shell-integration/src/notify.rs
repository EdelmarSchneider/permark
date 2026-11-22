//! The plugin's system call: telling Explorer that associations changed.

// Plain FFI calls into the Windows shell; each unsafe block says why it is sound.
#![allow(unsafe_code)]

use std::ptr;

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
