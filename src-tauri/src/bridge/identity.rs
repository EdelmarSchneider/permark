//! Who runs a process (Windows): the check that the program answering on a project's pipe is run by the same user as
//! the program asking.
//!
//! Anybody can create a pipe with a name that is free, and the names are machine-wide, so a program that connects to
//! `\\.\pipe\permark-<id>` has to find out who made it before it trusts what it hears. The pipe's server process id
//! comes from the connection (`GetNamedPipeServerProcessId`); this module reads the user in that process's token and
//! compares it with the user in ours.
//!
//! The only unsafe code of the agent connection is here: asking Windows for a token's user needs raw handles and a
//! buffer that Windows fills with a structure holding pointers. Each call is small and has its reason next to it.
//!
//! A process id can be reused once its process ends. The connection that the id came from keeps its server alive for
//! as long as it is open, which is what makes the id good for the check that follows it right away.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// A handle that is closed when it goes out of scope.
struct Handle(HANDLE);

impl Drop for Handle {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: the handle was returned to this value alone (a process or a token that was opened for it, never a
        // pseudo handle) and nothing else closes it.
        unsafe { CloseHandle(self.0) };
    }
}

/// The user of a process: the bytes of the security identifier in the process's token.
#[allow(unsafe_code)]
fn user_of(process: HANDLE) -> io::Result<Vec<u8>> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is a handle that is valid for the call, and `token` is a place for the handle it returns.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle(token);

    // The first call only asks how big the answer is: it "fails" and says so in `needed`.
    let mut needed = 0_u32;
    // SAFETY: a null buffer of length 0 is how the size is asked for; `needed` is a place for it.
    unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &raw mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // Eight-byte units, because the answer holds a pointer and has to be aligned for one.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    let mut written = 0_u32;
    // SAFETY: the buffer is at least `needed` bytes long, which is what the length says, and is aligned for the
    // structure Windows writes into it.
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
    // SAFETY: Windows wrote a `TOKEN_USER` at the start of the buffer, and the identifier it points to is inside the
    // same buffer, which lives until the end of this function. `GetLengthSid` reads that identifier's own header, and
    // the copy is of exactly the bytes it says the identifier has.
    unsafe {
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let sid = user.User.Sid;
        let length = GetLengthSid(sid) as usize;
        Ok(std::slice::from_raw_parts(sid.cast::<u8>(), length).to_vec())
    }
}

/// Whether the process `pid` runs as the same user as this program. An error when that cannot be found out (the process
/// is gone, or belongs to somebody whose processes are not ours to look into): such a process is not trusted either.
#[allow(unsafe_code)]
pub(crate) fn runs_as_this_user(pid: u32) -> io::Result<bool> {
    // SAFETY: the current process's pseudo handle is always valid and needs no closing.
    let ours = user_of(unsafe { GetCurrentProcess() })?;
    // SAFETY: opening a process by id has no preconditions; a null handle is the failure.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = Handle(process);
    Ok(user_of(process.0)? == ours)
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn this_program_runs_as_this_user() {
        assert!(runs_as_this_user(std::process::id()).expect("our own process can be looked at"));
    }

    #[test]
    fn a_program_this_one_started_runs_as_this_user() {
        let mut child = Command::new("cmd")
            .args(["/C", "pause"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start a process");
        let same = runs_as_this_user(child.id());
        child.kill().expect("stop it");
        let _ = child.wait();
        assert!(same.expect("a process of ours can be looked at"));
    }

    /// The System process belongs to another user (the system's), whatever the rights of the one running the tests: it
    /// is either refused outright or found to be somebody else's.
    #[test]
    fn a_process_of_the_system_does_not_run_as_this_user() {
        assert!(!matches!(runs_as_this_user(4), Ok(true)));
    }

    #[test]
    fn a_process_that_does_not_exist_is_not_trusted() {
        assert!(runs_as_this_user(0xFFFF_FFF0).is_err());
    }
}
