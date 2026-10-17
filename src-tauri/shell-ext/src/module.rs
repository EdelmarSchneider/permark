//! What the DLL knows about itself: where it is, and whether anything of it is still in use.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HMODULE, MAX_PATH};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};

/// How many COM objects of this DLL are alive, plus how many times a client asked to keep the DLL loaded
/// (`IClassFactory::LockServer`). `DllCanUnloadNow` answers from it.
static IN_USE: AtomicUsize = AtomicUsize::new(0);

/// A COM object of this DLL, or a lock on it, for as long as the value lives. Each object holds one, so the DLL stays
/// loaded while Windows can still call into it.
pub(crate) struct InUse;

impl InUse {
    pub(crate) fn new() -> Self {
        IN_USE.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for InUse {
    fn drop(&mut self) {
        IN_USE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Keeps the DLL loaded (`LockServer(TRUE)`) or lets it go again (`LockServer(FALSE)`).
pub(crate) fn lock_server(lock: bool) {
    if lock {
        IN_USE.fetch_add(1, Ordering::SeqCst);
    } else {
        // Never below zero, whatever a client does.
        let _ = IN_USE.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
    }
}

/// Nothing of the DLL is in use: Windows may unload it.
pub(crate) fn can_unload() -> bool {
    IN_USE.load(Ordering::SeqCst) == 0
}

/// The path of this DLL, as Windows loaded it. In the package that is `permark_shell.dll` in the package's folder.
pub(crate) fn path() -> Option<PathBuf> {
    // Any address inside this DLL names it.
    let inside: fn() -> Option<PathBuf> = path;
    let mut module = HMODULE::default();
    // SAFETY: with FROM_ADDRESS the "name" is an address inside the module to find; UNCHANGED_REFCOUNT means no handle is
    // left to release. `module` is a valid place to write the handle to.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(inside as *const u16),
            &raw mut module,
        )
        .ok()?;
    }
    module_path(module)
}

/// The path of `module`. The buffer grows until the whole path fits (a path may be longer than `MAX_PATH`).
fn module_path(module: HMODULE) -> Option<PathBuf> {
    let mut buffer = vec![0u16; MAX_PATH as usize];
    loop {
        // SAFETY: `buffer` is a valid, writable slice; the call writes at most its length.
        let written = unsafe { GetModuleFileNameW(Some(module), &mut buffer) } as usize;
        if written == 0 {
            return None;
        }
        if written < buffer.len() {
            return Some(PathBuf::from(OsString::from_wide(&buffer[..written])));
        }
        // Cut short: the path is longer than the buffer.
        if buffer.len() >= 32 * 1024 {
            return None;
        }
        let longer = buffer.len() * 2;
        buffer.resize(longer, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests run in one process on several threads and share the count; each one checks only its own difference.

    #[test]
    fn the_path_is_that_of_the_binary_the_code_is_in() {
        // In a test the code is in the test program, so that is the path.
        let found = path().expect("a path");
        let exe = std::env::current_exe().expect("test exe");
        assert_eq!(
            found.canonicalize().expect("exists"),
            exe.canonicalize().expect("exists")
        );
    }

    #[test]
    fn an_object_keeps_the_dll_in_use_until_it_is_gone() {
        let before = IN_USE.load(Ordering::SeqCst);
        let object = InUse::new();
        assert_eq!(IN_USE.load(Ordering::SeqCst), before + 1);
        drop(object);
        assert_eq!(IN_USE.load(Ordering::SeqCst), before);
    }

    #[test]
    fn a_client_can_keep_the_dll_loaded_and_let_go() {
        let object = InUse::new(); // keeps the count above zero while this test looks at it
        let before = IN_USE.load(Ordering::SeqCst);
        lock_server(true);
        assert_eq!(IN_USE.load(Ordering::SeqCst), before + 1);
        lock_server(false);
        assert_eq!(IN_USE.load(Ordering::SeqCst), before);
        drop(object);
    }
}
