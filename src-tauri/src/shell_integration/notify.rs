//! Tells the Windows shell that file associations changed, so Explorer redraws its menus and icons.

use std::ptr;

use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

/// Broadcasts "associations changed". It can take a moment when Explorer is busy, so callers run it off the UI.
// The one unsafe call of the app: a plain FFI call to `SHChangeNotify`, which reads no memory for this event
// (both item pointers are null, as the documentation asks for `SHCNE_ASSOCCHANGED`).
#[allow(unsafe_code)]
pub(super) fn associations_changed() {
    // SAFETY: `SHCNE_ASSOCCHANGED` takes no item pointers, and null is what the API documents for them.
    unsafe {
        #[allow(clippy::cast_possible_wrap)] // the event id is a small flag that fits an i32
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            ptr::null(),
            ptr::null(),
        );
    }
}
