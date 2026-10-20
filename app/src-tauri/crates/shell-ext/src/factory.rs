//! The class factory: COM asks the DLL for it (`DllGetClassObject`) and has it create the command (`CreateInstance`).

// What `#[implement]` writes (the COM glue around the struct) trips these pedantic lints.
#![allow(clippy::inline_always, clippy::ref_as_ptr)]

use std::ffi::c_void;

use windows::core::{implement, IUnknown, Interface, Ref, Result, BOOL, GUID};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_POINTER};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};

use crate::command::{MenuAction, MenuCommand};
use crate::guard::guarded;
use crate::module::{self, InUse};

#[implement(IClassFactory)]
pub(crate) struct Factory {
    action: MenuAction,
    _in_use: InUse,
}

impl Factory {
    pub(crate) fn new(action: MenuAction) -> Self {
        Self {
            action,
            _in_use: InUse::new(),
        }
    }
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        guarded(|| {
            if iid.is_null() || object.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: `object` is not null (checked above); an answer is written only on success.
            unsafe { *object = std::ptr::null_mut() };
            // the command is never part of another object
            if !outer.is_null() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let command: IUnknown = MenuCommand::new(self.action).into();
            // SAFETY: both pointers are valid (checked above); `query` writes an interface the caller then owns.
            unsafe { command.query(iid, object).ok() }
        })
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        module::lock_server(lock.as_bool());
        Ok(())
    }
}
