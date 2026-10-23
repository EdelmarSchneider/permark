//! Network folders (Windows): a share (`\\server\share`, `\\wsl$\...`, `\\wsl.localhost\...`, `\\?\UNC\...`) or a
//! mapped network drive. Their change notifications cannot be trusted (the WSL share accepts a watch and never
//! reports) -> no OS watch there: live updates are off, said so, and the open document is polled.

use std::path::Path;

/// `path` is on a network folder. Always `false` outside Windows.
#[must_use]
pub(crate) fn is_network(path: &Path) -> bool {
    #[cfg(windows)]
    {
        on_network(path, drive_is_remote)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// [`is_network`] with the drive check given: `remote(b'Z')` says whether drive `Z:` is a network drive (upper case).
#[cfg(windows)]
pub(crate) fn on_network(path: &Path, remote: impl Fn(u8) -> bool) -> bool {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return false;
    };
    match prefix.kind() {
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) => true,
        // `\\.\UNC\server\share`; `\\?\unc\...` (std may read a lower-case `unc` as a plain verbatim name)
        Prefix::DeviceNS(name) | Prefix::Verbatim(name) => name.eq_ignore_ascii_case("UNC"),
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => remote(letter.to_ascii_uppercase()),
    }
}

/// `GetDriveTypeW` result of a network drive (`WindowsProgramming::DRIVE_REMOTE`; that feature for one constant).
#[cfg(windows)]
const DRIVE_REMOTE: u32 = 4;

/// Drive `letter:` is a mapped network drive.
// Documented FFI: std has no drive-type query. `root` is a NUL-terminated wide string owned by this function and
// alive for the whole call.
#[cfg(windows)]
#[allow(unsafe_code)]
fn drive_is_remote(letter: u8) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
    // SAFETY: `root` is a live NUL-terminated buffer that outlives the call.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    kind == DRIVE_REMOTE
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn local(_: u8) -> bool {
        false
    }

    #[test]
    fn shares_are_network_folders_whatever_their_form() {
        for path in [
            r"\\server\share",
            r"\\server\share\docs\a.md",
            r"\\wsl$\Ubuntu-24.04\tmp\pm07wsl",
            r"\\wsl.localhost\Ubuntu-24.04\home\edelm\cursos",
            r"\\?\UNC\wsl.localhost\Ubuntu-24.04\tmp",
            r"\\?\UNC\server\share\dir",
            r"\\.\UNC\server\share\dir",
        ] {
            assert!(on_network(Path::new(path), local), "{path}");
        }
    }

    #[test]
    fn local_drives_are_not() {
        for path in [
            r"C:\",
            r"C:\Users\me\docs",
            r"\\?\C:\Users\me\docs",
            r"d:\x",
        ] {
            assert!(!on_network(Path::new(path), local), "{path}");
        }
        assert!(!is_network(&std::env::temp_dir()));
        assert!(!is_network(Path::new(r"C:\")));
    }

    #[test]
    fn odd_forms_of_a_share_still_count() {
        for path in [
            r"\\WSL.LOCALHOST\Ubuntu\tmp",
            r"\\Wsl$\Ubuntu\",
            r"\\wsl.localhost\Ubuntu-24.04\tmp\",
            r"\\server\share\",
            "//server/share/docs",
            r"\\?\unc\server\share",
            r"\\.\unc\server\share",
            "\\\\sérvidor\\pasta-ç\\文档",
            r"\\server\share with space\a b",
        ] {
            assert!(on_network(Path::new(path), local), "{path}");
        }
        // pure: no network is touched to say so
        assert!(is_network(Path::new(r"\\wsl.localhost\NoSuchDistro\x")));
        assert!(is_network(Path::new(r"\\no-such-host.invalid\share")));
    }

    #[test]
    fn a_mapped_drive_asks_the_drive_type_with_its_letter() {
        let asked = std::cell::RefCell::new(Vec::new());
        let z_is_remote = |letter: u8| {
            asked.borrow_mut().push(letter);
            letter == b'Z'
        };
        assert!(on_network(Path::new(r"Z:\projects\a"), z_is_remote));
        assert!(on_network(Path::new(r"z:\projects\a"), z_is_remote));
        assert!(on_network(Path::new(r"\\?\Z:\projects"), z_is_remote));
        assert!(!on_network(Path::new(r"C:\projects"), z_is_remote));
        assert_eq!(*asked.borrow(), vec![b'Z', b'Z', b'Z', b'C']);
    }

    #[test]
    fn shares_never_ask_the_drive_type_and_other_forms_are_local() {
        let never = |_: u8| -> bool { panic!("no drive to ask") };
        assert!(on_network(Path::new(r"\\server\share"), never));
        for path in [
            r"\\?\Volume{0b1e2c3d-0000-0000-0000-100000000000}\docs",
            r"\\.\PhysicalDrive0",
            r"relative\docs",
            "",
            r"\docs",
        ] {
            assert!(!on_network(Path::new(path), never), "{path}");
        }
    }
}
