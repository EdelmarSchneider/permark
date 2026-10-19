//! macOS: default handlers through `LaunchServices`, one content type (UTI) at a time.
//!
//! The only unsafe code of this module: plain C calls into `CoreFoundation` and `CoreServices`. Every `CFStringRef` made
//! here is owned by a `CfString`, which releases it when dropped; a call that hands back a string we own ("Create" /
//! "Copy" in its name) is wrapped at once.

// FFI with the system frameworks; each unsafe block says why it is sound.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_void, CStr};

use super::{FileGroup, Handlers};
use crate::{ErrorKind, IpcError};

type CFStringRef = *const c_void;
type CFIndex = isize;
type OSStatus = i32;

const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8
const ROLES_ALL: u32 = 0xFFFF_FFFF; // kLSRolesAll

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        count: CFIndex,
        encoding: u32,
        external: u8,
    ) -> CFStringRef;
    fn CFStringGetCString(s: CFStringRef, buffer: *mut c_char, size: CFIndex, encoding: u32) -> u8;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    static kUTTagClassFilenameExtension: CFStringRef;
    fn UTTypeCreatePreferredIdentifierForTag(
        class: CFStringRef,
        tag: CFStringRef,
        conforming_to: CFStringRef,
    ) -> CFStringRef;
    fn LSCopyDefaultRoleHandlerForContentType(content_type: CFStringRef, role: u32) -> CFStringRef;
    fn LSSetDefaultRoleHandlerForContentType(
        content_type: CFStringRef,
        role: u32,
        handler: CFStringRef,
    ) -> OSStatus;
}

/// A `CFStringRef` this code owns (released on drop). Never null.
struct CfString(CFStringRef);

impl CfString {
    fn new(text: &str) -> Option<Self> {
        let count = CFIndex::try_from(text.len()).ok()?;
        // SAFETY: `text` is valid UTF-8 of `count` bytes for the duration of the call, which copies it.
        let made =
            unsafe { CFStringCreateWithBytes(std::ptr::null(), text.as_ptr(), count, UTF8, 0) };
        Self::owned(made)
    }

    /// Takes ownership of a string a "Create" | "Copy" call returned (`None` for null).
    fn owned(made: CFStringRef) -> Option<Self> {
        (!made.is_null()).then_some(Self(made))
    }

    fn to_text(&self) -> Option<String> {
        let mut buffer = [0 as c_char; 1024];
        let size = CFIndex::try_from(buffer.len()).ok()?;
        // SAFETY: `self.0` is a live string; the call writes at most `size` bytes (NUL included) into `buffer`.
        let ok = unsafe { CFStringGetCString(self.0, buffer.as_mut_ptr(), size, UTF8) };
        if ok == 0 {
            return None;
        }
        // SAFETY: on success the buffer holds a NUL-terminated string.
        let text = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        text.to_str().ok().map(str::to_owned)
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        // SAFETY: owned and not null (see `owned`); released once.
        unsafe { CFRelease(self.0) };
    }
}

/// The content type of a file extension (without its dot): the declared UTI, else the dynamic one.
fn uti_of(extension: &str) -> Option<String> {
    let tag = CfString::new(extension)?;
    // SAFETY: a constant of the framework, read once; `tag` is live for the call, which returns a string we own.
    let made = unsafe {
        UTTypeCreatePreferredIdentifierForTag(kUTTagClassFilenameExtension, tag.0, std::ptr::null())
    };
    CfString::owned(made)?.to_text()
}

/// Default handlers recorded by `LaunchServices`; PerMark is `bundle` (its bundle identifier).
pub struct LaunchServices {
    bundle: String,
}

impl LaunchServices {
    #[must_use]
    pub fn new(bundle: impl Into<String>) -> Self {
        Self {
            bundle: bundle.into(),
        }
    }
}

impl Handlers for LaunchServices {
    fn keys(&self, group: FileGroup) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for extension in group.extensions() {
            if let Some(uti) = uti_of(extension.trim_start_matches('.')) {
                if !keys.contains(&uti) {
                    keys.push(uti);
                }
            }
        }
        keys
    }

    fn get(&self, key: &str) -> Option<String> {
        let content_type = CfString::new(key)?;
        // SAFETY: `content_type` is live for the call, which returns a string we own (or null).
        let made = unsafe { LSCopyDefaultRoleHandlerForContentType(content_type.0, ROLES_ALL) };
        CfString::owned(made)?.to_text()
    }

    fn set(&self, key: &str, handler: Option<&str>) -> Result<(), IpcError> {
        // `LaunchServices` has no "no choice": giving back "none" leaves the type to the system's own pick next time.
        let Some(handler) = handler else {
            return Ok(());
        };
        let refused = || IpcError::new(ErrorKind::Denied).subject(key.to_owned());
        let content_type = CfString::new(key).ok_or_else(refused)?;
        let bundle = CfString::new(handler).ok_or_else(refused)?;
        // SAFETY: both strings are live for the call, which only reads them.
        let status =
            unsafe { LSSetDefaultRoleHandlerForContentType(content_type.0, ROLES_ALL, bundle.0) };
        if status == 0 {
            Ok(())
        } else {
            Err(refused().detail(format!("LaunchServices status {status}")))
        }
    }

    fn me(&self) -> &str {
        &self.bundle
    }
}
