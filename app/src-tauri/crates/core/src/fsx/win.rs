//! Windows-only: rename with replace + write-through (std rename does not ask for write-through).

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf, Prefix};

/// Paths of this many UTF-16 units or more get `\\?\` (Win32 reads at most 259 + NUL without it).
const SHORT_PATH_UNITS: usize = 240;

/// `MoveFileExW(from, to, REPLACE_EXISTING | WRITE_THROUGH)`.
// Documented FFI: MoveFileExW has no safe std wrapper with WRITE_THROUGH. Both arguments are NUL-terminated wide
// strings owned by this function and alive for the whole call.
#[allow(unsafe_code)]
pub(super) fn move_replace_write_through(from: &Path, to: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let (from, to) = (wide(&extended(from))?, wide(&extended(to))?);
    // SAFETY: both pointers come from live NUL-terminated buffers that outlive the call.
    let moved = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// `(volume serial, file index)` of `path`; a link | junction is opened itself (`OPEN_REPARSE_POINT`). The handle
/// (no access asked, every share mode) lives for this call only: never in the way of a rename.
// Documented FFI: GetFileInformationByHandle has no stable std wrapper (`MetadataExt::file_index` is unstable). The
// handle is owned by `file`, alive for the whole call; `info` is a plain out struct written by the call.
#[allow(unsafe_code)]
pub(super) fn file_identity(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(extended(path))?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` keeps the handle open for the call; `info` is a live, writable struct of the expected type.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    units.push(0);
    Ok(units)
}

/// Long absolute path -> `\\?\C:\...` | `\\?\UNC\server\share\...`; anything else unchanged.
fn extended(path: &Path) -> PathBuf {
    if path.as_os_str().encode_wide().count() < SHORT_PATH_UNITS || !path.is_absolute() {
        return path.to_path_buf();
    }
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path.to_path_buf();
    };
    let mut out = OsString::from(r"\\?\");
    match prefix.kind() {
        Prefix::Disk(letter) => out.push(format!("{}:", char::from(letter))),
        Prefix::UNC(server, share) => {
            out.push(r"UNC\");
            out.push(server);
            out.push(r"\");
            out.push(share);
        }
        _ => return path.to_path_buf(),
    }
    for component in components {
        match component {
            Component::Normal(name) => {
                out.push(r"\");
                out.push(name);
            }
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => return path.to_path_buf(),
        }
    }
    out.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_long_absolute_paths_get_the_prefix() {
        assert_eq!(
            extended(Path::new(r"C:\a\b.md")),
            PathBuf::from(r"C:\a\b.md")
        );
        let long = format!(r"C:\{}\x.md", "d".repeat(300));
        assert!(extended(Path::new(&long))
            .to_string_lossy()
            .starts_with(r"\\?\C:\"));
        let unc = format!(r"\\srv\share\{}", "d".repeat(300));
        assert!(extended(Path::new(&unc))
            .to_string_lossy()
            .starts_with(r"\\?\UNC\srv\share\"));
        let dotted = format!(r"C:\{}\..\x", "d".repeat(300));
        assert_eq!(extended(Path::new(&dotted)), PathBuf::from(&dotted));
        let already = format!(r"\\?\C:\{}", "d".repeat(300));
        assert_eq!(extended(Path::new(&already)), PathBuf::from(&already));
    }
}
