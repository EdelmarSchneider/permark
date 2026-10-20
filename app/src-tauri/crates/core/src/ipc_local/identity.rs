//! Windows: does a process run as the same user as this one (token user SID compare)? The only unsafe code of
//! `ipc_local`: token queries need raw handles + a buffer Windows fills. A pid from a live pipe connection stays
//! valid while the connection is open (its process cannot exit + be reused meanwhile).

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Handle closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    // Documented FFI: closes a handle owned by this value alone.
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: the handle was opened for this value (never a pseudo handle); nothing else closes it.
        unsafe { CloseHandle(self.0) };
    }
}

/// User SID bytes of a process token.
// Documented FFI: token query (size probe, then fill an aligned buffer, then copy the SID it points into).
#[allow(unsafe_code)]
fn user_of(process: HANDLE) -> io::Result<Vec<u8>> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` valid for the call; `token` is a place for the returned handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle(token);
    let mut needed = 0_u32;
    // SAFETY: null buffer + length 0 = size probe; `needed` receives the size.
    unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &raw mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 units: the structure holds a pointer and must be aligned for one
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    let mut written = 0_u32;
    // SAFETY: buffer is >= `needed` bytes and pointer-aligned.
    let filled = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut written,
        )
    };
    if filled == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Windows wrote a TOKEN_USER at the buffer start; its SID points inside the same live buffer;
    // GetLengthSid reads the SID's own header; the copy is exactly that many bytes.
    unsafe {
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let sid = user.User.Sid;
        let length = GetLengthSid(sid) as usize;
        Ok(std::slice::from_raw_parts(sid.cast::<u8>(), length).to_vec())
    }
}

/// Whether process `pid` runs as this user. Error when it cannot be told (gone | not ours to inspect): untrusted.
// Documented FFI: process handle for a token query.
#[allow(unsafe_code)]
pub(super) fn runs_as_this_user(pid: u32) -> io::Result<bool> {
    // SAFETY: the current-process pseudo handle is always valid and needs no closing.
    let ours = user_of(unsafe { GetCurrentProcess() })?;
    // SAFETY: OpenProcess has no preconditions; null = failure.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = Handle(process);
    Ok(user_of(process.0)? == ours)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_runs_as_this_user() {
        assert!(runs_as_this_user(std::process::id()).unwrap());
    }

    #[test]
    fn the_system_process_is_not_us() {
        assert!(!matches!(runs_as_this_user(4), Ok(true)));
    }

    #[test]
    fn a_missing_process_is_not_trusted() {
        assert!(runs_as_this_user(0xFFFF_FFF0).is_err());
    }
}
