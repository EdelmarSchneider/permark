//! "Open with PerMark" in the right-click menu of folders, for the Microsoft Store version of PerMark.
//!
//! A packaged program cannot add a command to the Explorer menu through the registry (what it writes there goes to a
//! private copy that Explorer never reads). Windows has a way for packages: the package's manifest names a COM class that
//! implements `IExplorerCommand`, and says for which items it is on the menu. This DLL is that class. The manifest
//! (`src-tauri/msstore/AppxManifest.xml`) declares it as a surrogate server, so Windows loads the DLL into its own helper
//! process (`dllhost.exe`) and not into Explorer: what goes wrong in here cannot take Explorer down. `docs/msstore.md` has
//! the whole picture.
//!
//! The DLL sits in the package's folder next to `permark.exe`, and finds the program from its own path. It does not
//! touch the registry and has nothing to register: Windows reads the manifest.
//!
//! Plain Rust where it can be ([`labels`], [`launch`]); the rest is the COM and shell glue, which is unsafe code in a few
//! small places, each with a comment on why it is sound. No panic leaves the DLL (see [`guard`]).

#![cfg(windows)]
// The two exported functions are named in the DLL's export table on purpose; the linker's note that they could be marked
// PRIVATE is about import libraries, which nothing here makes use of.
#![allow(linker_messages)]

mod command;
mod factory;
mod guard;
mod labels;
mod launch;
mod module;
mod shell;

use std::ffi::c_void;

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Com::IClassFactory;

use crate::factory::Factory;
use crate::guard::guarded_code;

/// The class id of the command. It is in `AppxManifest.xml` too (a test below checks that the two agree), and it is not to
/// change: it is how Windows knows the command from one version of the package to the next.
pub const CLSID_OPEN_WITH_PERMARK: GUID =
    GUID::from_u128(0xD5FF_B935_7A42_4E3D_90E1_0180_A799_9B94);

/// The class id of "New Markdown file with PerMark". It is in `AppxManifest.xml` too, and it is not to change.
pub const CLSID_NEW_MARKDOWN: GUID = GUID::from_u128(0xC3B7_E2A1_9D54_4C18_B6F0_2E8A_71D4_5C09);

/// Where COM gets the factory of a class from this DLL.
///
/// # Safety
///
/// Called by COM: `class` and `interface` point to ids, and `object` to a place to write the factory to, or are null.
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
        let (class, interface) = unsafe {
            *object = std::ptr::null_mut();
            (&*class, interface)
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
mod tests {
    use super::*;

    #[test]
    fn the_class_id_is_the_one_in_the_manifest() {
        const MANIFEST: &str = include_str!("../../msstore/AppxManifest.xml");
        let id = format!("{CLSID_OPEN_WITH_PERMARK:?}").to_uppercase();
        assert_eq!(id, "D5FFB935-7A42-4E3D-90E1-0180A7999B94");
        assert!(
            MANIFEST.matches(&id).count() >= 2,
            "the manifest declares the class and registers the command for it"
        );
        let new_id = format!("{CLSID_NEW_MARKDOWN:?}").to_uppercase();
        assert_eq!(new_id, "C3B7E2A1-9D54-4C18-B6F0-2E8A71D45C09");
        assert!(
            MANIFEST.matches(&new_id).count() >= 2,
            "the manifest declares the new-file class and registers it"
        );
    }

    #[test]
    fn an_unknown_class_has_no_factory() {
        let other = GUID::from_u128(0x1234);
        let mut object = std::ptr::null_mut();
        // SAFETY: valid ids and a valid place for the answer.
        let code =
            unsafe { DllGetClassObject(&raw const other, &IClassFactory::IID, &raw mut object) };
        assert_eq!(code, CLASS_E_CLASSNOTAVAILABLE);
        assert!(object.is_null());
    }

    #[test]
    fn null_pointers_are_an_error_and_not_a_crash() {
        // SAFETY: the function checks for null.
        let code =
            unsafe { DllGetClassObject(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
        assert_eq!(code, E_POINTER);
    }
}
