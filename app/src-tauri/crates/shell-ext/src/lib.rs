//! "Open with PerMark" and "New Markdown file" in the right-click menu of folders, for the Microsoft Store package.
//!
//! A packaged program cannot add menu entries through the registry (its writes go to a private copy Explorer never
//! reads). The package manifest names COM classes that implement `IExplorerCommand` instead, and Windows loads this DLL
//! for them, in its own helper process (`dllhost.exe`, a surrogate server), never inside Explorer. The DLL sits next to
//! `permark.exe` in the package folder and finds the program from its own path; it registers nothing.
//!
//! Plain Rust where it can be ([`labels`], [`launch`]); the rest is COM glue with a few small unsafe blocks, each
//! commented. No panic crosses the boundary: every entry point runs through `guard`, and the DLL is built with the
//! `release-shell` profile, which unwinds so the guard can catch.

#![cfg(windows)]
// A COM server is FFI end to end: unsafe code in a few small, commented places.
#![allow(unsafe_code)]
// The two exports are named in the DLL's export table on purpose (the linker's note is about import libraries).
#![allow(linker_messages)]

mod command;
mod factory;
mod guard;
pub mod labels;
pub mod launch;
mod module;
mod shell;

use std::ffi::c_void;

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Com::IClassFactory;

use crate::factory::Factory;
use crate::guard::guarded_code;

/// "Open with PerMark". Also in `msstore/AppxManifest.xml`; never changes (Windows knows the command by it).
pub const CLSID_OPEN_WITH_PERMARK: GUID =
    GUID::from_u128(0xD5FF_B935_7A42_4E3D_90E1_0180_A799_9B94);

/// "New Markdown file with PerMark". Also in the manifest; never changes.
pub const CLSID_NEW_MARKDOWN: GUID = GUID::from_u128(0xC3B7_E2A1_9D54_4C18_B6F0_2E8A_71D4_5C09);

/// Where COM gets the factory of a class of this DLL.
///
/// # Safety
/// Called by COM: `class` and `interface` point to ids, `object` to a place for the answer, or they are null.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    interface: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    guarded_code(|| {
        if class.is_null() || interface.is_null() || object.is_null() {
            return E_POINTER;
        }
        // SAFETY: not null (checked above); the caller passes ids and a place for the answer.
        let class = unsafe {
            *object = std::ptr::null_mut();
            &*class
        };
        let Some(action) = command::MenuAction::from_class(class) else {
            return CLASS_E_CLASSNOTAVAILABLE;
        };
        let factory: IClassFactory = Factory::new(action).into();
        // SAFETY: `interface` and `object` are valid (checked above); `query` writes an interface the caller then owns.
        unsafe { factory.query(interface, object) }
    })
}

/// Whether Windows may unload the DLL: nothing of it is in use.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    guarded_code(|| if module::can_unload() { S_OK } else { S_FALSE })
}

#[cfg(test)]
mod tests;
