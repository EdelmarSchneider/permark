//! macOS: memory and processor kind through `sysctlbyname` (libSystem). Free memory left unknown (v1).
//!
//! The only unsafe code of this module: plain `sysctlbyname` reads into buffers this module owns.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_int, c_void, CStr};

use super::{Gpu, MachineProfile};

extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
}

pub(super) fn read(profile: &mut MachineProfile) {
    profile.ram_total = int(c"hw.memsize");
    let brand = string(c"machdep.cpu.brand_string");
    let apple = int(c"hw.optional.arm64") == Some(1)
        || brand.as_deref().is_some_and(|b| b.starts_with("Apple"));
    if apple {
        profile.gpus = vec![Gpu {
            name: brand.unwrap_or_else(|| "Apple".to_owned()),
            dedicated_vram: None,
            shared: None,
            unified: true,
            software: false,
        }];
    }
}

/// Integer value (4 | 8 bytes) of `name`; `None`: absent | other size.
fn int(name: &CStr) -> Option<u64> {
    let mut buf = [0u8; 8];
    let mut len = buf.len();
    // SAFETY: `name` is NUL-ended; `buf` is writable for `len` bytes and `len` is updated by the call; no new value.
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    match (rc, len) {
        (0, 8) => Some(u64::from_ne_bytes(buf)),
        (0, 4) => Some(u64::from(u32::from_ne_bytes([
            buf[0], buf[1], buf[2], buf[3],
        ]))),
        _ => None,
    }
}

/// Text value of `name` (NUL-ended by the system).
fn string(name: &CStr) -> Option<String> {
    let mut len = 0usize;
    // SAFETY: size query: null buffer, `len` receives the needed size.
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 || len > 4096 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: `buf` is writable for `len` bytes; the call writes at most `len` and updates it.
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(len);
    let text = CStr::from_bytes_until_nul(&buf).ok()?.to_str().ok()?;
    Some(text.trim().to_owned())
}
