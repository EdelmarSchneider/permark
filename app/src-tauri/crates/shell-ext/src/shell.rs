//! The few things asked of Windows and the shell, one small function each.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::core::{Interface, Result, PCWSTR, PWSTR};
use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::Win32::System::Com::{CoTaskMemFree, IServiceProvider};
use windows::Win32::UI::Shell::{
    IFolderView, IShellItem, IShellItemArray, SHStrDupW, SIGDN_FILESYSPATH,
};

/// `text` in memory the caller frees with `CoTaskMemFree` (how the shell wants every string a command hands it).
pub(crate) fn alloc_string(text: &str) -> Result<PWSTR> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call, which copies it.
    unsafe { SHStrDupW(PCWSTR(wide.as_ptr())) }
}

/// The person's display languages, best first (`de-DE`); empty when Windows does not say.
pub(crate) fn display_languages() -> Vec<String> {
    let mut count = 0u32;
    let mut length = 0u32;
    // SAFETY: the first call only asks for the length (no buffer); the second fills a buffer of exactly that length.
    // The list is NUL-separated tags closed by an empty one.
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &raw mut count, None, &raw mut length)
            .is_err()
            || length == 0
        {
            return Vec::new();
        }
        let mut buffer = vec![0u16; length as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &raw mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &raw mut length,
        )
        .is_err()
        {
            return Vec::new();
        }
        buffer
            .split(|&unit| unit == 0)
            .filter(|tag| !tag.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

/// An item's file system path (`None` for a virtual item).
pub(crate) fn file_system_path(item: &IShellItem) -> Option<PathBuf> {
    // SAFETY: on success the call hands over a NUL-terminated string the caller frees: copied, then freed.
    unsafe {
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(OsString::from_wide(name.as_wide()));
        CoTaskMemFree(Some(name.0.cast_const().cast()));
        Some(path)
    }
}

/// How many items are selected (`None`: no array, as for the background of a folder).
pub(crate) fn count(items: Option<&IShellItemArray>) -> Option<u32> {
    // SAFETY: a plain call on a live interface.
    items.and_then(|items| unsafe { items.GetCount().ok() })
}

/// The selected items' paths, in order; items without one are left out.
pub(crate) fn selected_paths(items: Option<&IShellItemArray>) -> Vec<PathBuf> {
    let Some(items) = items else {
        return Vec::new();
    };
    let total = count(Some(items)).unwrap_or(0);
    (0..total)
        // SAFETY: a plain call on a live interface; an index past the end is an error, not a crash.
        .filter_map(|index| unsafe { items.GetItemAt(index).ok() })
        .filter_map(|item| file_system_path(&item))
        .collect()
}

/// The folder an Explorer window shows, from the site Explorer handed the command: how a command on the background of a
/// folder learns which folder (the selection is empty then).
pub(crate) fn folder_of_site(site: &windows::core::IUnknown) -> Option<PathBuf> {
    let provider: IServiceProvider = site.cast().ok()?;
    // the folder view's service id is its interface id (`SID_SFolderView` = `IID_IFolderView`)
    // SAFETY: plain calls on live interfaces.
    unsafe {
        let view: IFolderView = provider.QueryService(&IFolderView::IID).ok()?;
        let folder: IShellItem = view.GetFolder().ok()?;
        file_system_path(&folder)
    }
}
