//! Nothing that goes wrong in here may reach the caller as a panic. The caller is File Explorer's COM surrogate (and a
//! panic that unwinds out of a function Windows called is the end of that process), so every entry point runs its work
//! through here and answers with an error code instead.

use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::core::{Error, Result, HRESULT};
use windows::Win32::Foundation::E_FAIL;

/// Runs `work`; a panic in it becomes `E_FAIL`. Needs the DLL to be built to unwind on a panic (`panic = "unwind"`, which
/// its release profile says), or there is nothing to catch.
pub(crate) fn guarded<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| Err(Error::from(E_FAIL)))
}

/// The same for the two functions the DLL exports, which answer with a bare `HRESULT`.
pub(crate) fn guarded_code(work: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or(E_FAIL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{E_POINTER, S_OK};

    #[test]
    fn the_answer_of_work_that_goes_well_is_passed_on() {
        assert_eq!(guarded(|| Ok(7)).ok(), Some(7));
        assert_eq!(guarded_code(|| S_OK), S_OK);
    }

    #[test]
    fn an_error_is_passed_on_as_it_is() {
        let error = guarded::<()>(|| Err(Error::from(E_POINTER))).expect_err("an error");
        assert_eq!(error.code(), E_POINTER);
    }

    #[test]
    fn a_panic_is_an_error_code_and_not_a_crash() {
        let error = guarded::<()>(|| panic!("a bug")).expect_err("an error");
        assert_eq!(error.code(), E_FAIL);
        assert_eq!(guarded_code(|| panic!("a bug")), E_FAIL);
    }
}
