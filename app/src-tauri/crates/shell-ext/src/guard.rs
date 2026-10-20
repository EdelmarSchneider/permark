//! No panic reaches the caller (the COM surrogate): every entry point runs its work here and answers with an error code
//! instead. Needs a build that unwinds (`release-shell` profile), or there is nothing to catch.

use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::core::{Error, Result, HRESULT};
use windows::Win32::Foundation::E_FAIL;

/// Runs `work`; a panic in it becomes `E_FAIL`.
pub(crate) fn guarded<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| Err(Error::from(E_FAIL)))
}

/// The same for the two exported functions, which answer with a bare `HRESULT`.
pub(crate) fn guarded_code(work: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or(E_FAIL)
}
