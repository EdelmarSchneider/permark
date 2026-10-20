//! Windows: how many bytes a pipe holds, without reading or blocking. A pipe in no-wait mode reports "no data" with
//! the same error as a closed pipe (both read as end of file through `Read`), so a deadline read asks first.

use std::io;
use std::os::windows::io::{AsHandle, AsRawHandle};
use std::ptr;

use interprocess::local_socket::Stream;
use windows_sys::Win32::System::Pipes::PeekNamedPipe;

/// Bytes waiting in the pipe; an error when the peer is gone.
// Documented FFI: `PeekNamedPipe` with only the "bytes available" out-parameter (no buffer is read).
#[allow(unsafe_code)]
pub(super) fn available(stream: &Stream) -> io::Result<u32> {
    let Stream::NamedPipe(pipe) = stream;
    let handle = pipe.as_handle().as_raw_handle();
    let mut avail: u32 = 0;
    // SAFETY: the handle is borrowed from a live stream for the length of the call; null buffer + size 0 means
    // nothing is copied; `avail` is a valid u32 out-parameter.
    let ok = unsafe {
        PeekNamedPipe(
            handle,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &raw mut avail,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(avail)
    }
}
