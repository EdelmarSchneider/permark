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

/// Live COM objects of this DLL + client locks (`IClassFactory::LockServer`). `DllCanUnloadNow` answers from it.
static IN_USE: AtomicUsize = AtomicUsize::new(0);

/// One COM object of this DLL (or a lock) for as long as the value lives: the DLL stays loaded meanwhile.
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

/// `LockServer(TRUE)` keeps the DLL loaded, `LockServer(FALSE)` lets it go (never below zero, whatever a client does).
pub(crate) fn lock_server(lock: bool) {
    if lock {
        IN_USE.fetch_add(1, Ordering::SeqCst);
    } else {
        let _ = IN_USE.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
    }
}

/// Nothing of the DLL is in use.
pub(crate) fn can_unload() -> bool {
    IN_USE.load(Ordering::SeqCst) == 0
}

/// The path of this DLL as Windows loaded it (in the package: `permark_shell.dll` in the package folder).
pub(crate) fn path() -> Option<PathBuf> {
    // any address inside this DLL names it
    let inside: fn() -> Option<PathBuf> = path;
    let mut module = HMODULE::default();
    // SAFETY: with FROM_ADDRESS the "name" is an address inside the module; UNCHANGED_REFCOUNT leaves no handle to
    // release. `module` is a valid place for the handle.
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

/// The path of `module`; the buffer grows until the whole path fits (paths may exceed `MAX_PATH`).
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
        if buffer.len() >= 32 * 1024 {
            return None;
        }
        let longer = buffer.len() * 2;
        buffer.resize(longer, 0);
    }
}
