//! WSL folders (Windows): `\\wsl$\<distro>\...` | `\\wsl.localhost\<distro>\...` in any spelling (`\\?\UNC\`,
//! `\\.\UNC\`, `//`, any case), or a drive letter mapped | substituted to one. The WSL share accepts a watch and never
//! reports -> no OS watch there: live updates are off, said so, and the open document is polled. Other shares (SMB)
//! and mapped network drives keep the OS watch: it usually works there.

use std::path::Path;

/// `path` is in a WSL folder. Always `false` outside Windows.
#[must_use]
pub(crate) fn is_wsl(path: &Path) -> bool {
    #[cfg(windows)]
    {
        on_wsl(path, drive_target)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// [`is_wsl`] with the drive lookup given: `target(b'W')` = what drive `W:` points to (`QueryDosDeviceW`), `None`:
/// nothing.
#[cfg(windows)]
pub(crate) fn on_wsl(path: &Path, target: impl Fn(u8) -> Option<String>) -> bool {
    use std::path::{Component, Prefix};
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return false;
    };
    match prefix.kind() {
        Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _) => {
            server.to_str().is_some_and(is_wsl_host)
        }
        // `\\.\UNC\server\...`; `\\?\unc\...` (std may read a lower-case `unc` as a plain verbatim name): the server
        // is the first name after the prefix
        Prefix::DeviceNS(name) | Prefix::Verbatim(name) if name.eq_ignore_ascii_case("UNC") => {
            components
                .find_map(|c| match c {
                    Component::Normal(server) => Some(server),
                    _ => None,
                })
                .and_then(|server| server.to_str())
                .is_some_and(is_wsl_host)
        }
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            target(letter.to_ascii_uppercase()).is_some_and(|t| target_is_wsl(&t))
        }
        Prefix::DeviceNS(_) | Prefix::Verbatim(_) => false,
    }
}

/// The host of the WSL share.
#[cfg(windows)]
fn is_wsl_host(server: &str) -> bool {
    server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost")
}

/// A drive's device target points into WSL: the Plan 9 redirector WSL serves its share with, or a WSL host as the
/// server (`\Device\<redirector>\;W:<logon>\wsl.localhost\<distro>` for a mapped drive, `\??\UNC\wsl$\<distro>\...`
/// for `subst`).
#[cfg(windows)]
fn target_is_wsl(target: &str) -> bool {
    let segments: Vec<&str> = target.split('\\').collect();
    segments
        .iter()
        .any(|s| s.trim_start_matches(';').eq_ignore_ascii_case("P9Rdr"))
        || segments.windows(2).any(|pair| {
            (pair[0].starts_with(';') || pair[0].eq_ignore_ascii_case("UNC"))
                && is_wsl_host(pair[1])
        })
}

/// What drive `letter:` points to (first target), `None`: no such drive.
// Documented FFI: std has no DOS-device query. `name` is a NUL-terminated wide string and `out` a writable buffer
// of the length passed, both owned by this function and alive for the whole call.
#[cfg(windows)]
#[allow(unsafe_code)]
fn drive_target(letter: u8) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::QueryDosDeviceW;
    let name = [u16::from(letter), u16::from(b':'), 0];
    let mut out = vec![0u16; 1024];
    let len = u32::try_from(out.len()).unwrap_or(u32::MAX);
    // SAFETY: `name` is a live NUL-terminated buffer; `out` is live, writable and `len` units long.
    let written = unsafe { QueryDosDeviceW(name.as_ptr(), out.as_mut_ptr(), len) };
    if written == 0 {
        return None;
    }
    let first = out.split(|&unit| unit == 0).next().unwrap_or_default();
    Some(String::from_utf16_lossy(first))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn no_drive(_: u8) -> Option<String> {
        None
    }

    #[test]
    fn wsl_shares_are_wsl_whatever_their_form() {
        for path in [
            r"\\wsl$\Ubuntu-24.04\tmp\pm07wsl",
            r"\\wsl.localhost\Ubuntu-24.04\home\edelm\cursos",
            r"\\?\UNC\wsl.localhost\Ubuntu-24.04\tmp",
            r"\\?\UNC\wsl$\Ubuntu\tmp",
            r"\\.\UNC\wsl.localhost\Ubuntu\tmp",
            "//wsl$/Ubuntu/tmp",
            "//wsl.localhost/Ubuntu/tmp",
        ] {
            assert!(on_wsl(Path::new(path), no_drive), "{path}");
        }
    }

    #[test]
    fn other_shares_and_local_drives_keep_the_watch() {
        for path in [
            r"\\server\share",
            r"\\server\share\docs\a.md",
            r"\\?\UNC\server\share\dir",
            r"\\.\UNC\server\share",
            "//carbon/Shared",
            r"\\192.168.0.113\midia",
            r"C:\",
            r"C:\Users\me\docs",
            r"\\?\C:\Users\me\docs",
        ] {
            assert!(!on_wsl(Path::new(path), no_drive), "{path}");
        }
        assert!(!is_wsl(&std::env::temp_dir()));
        assert!(!is_wsl(Path::new(r"C:\")));
        assert!(!is_wsl(Path::new(r"\\server\share")));
    }

    #[test]
    fn odd_spellings_of_a_wsl_share_still_count() {
        for path in [
            r"\\WSL.LOCALHOST\Ubuntu\tmp",
            r"\\Wsl$\Ubuntu\",
            r"\\wsl.localhost\Ubuntu-24.04\tmp\",
            r"\\wsl.localhost\Ubuntu",
            r"\\?\unc\wsl.localhost\Ubuntu",
            r"\\.\unc\WSL$\Ubuntu",
            r"\\wsl.localhost\Ubuntu\home\josé\文档 notes",
        ] {
            assert!(on_wsl(Path::new(path), no_drive), "{path}");
        }
        // pure: no network is touched to say so
        assert!(is_wsl(Path::new(r"\\wsl.localhost\NoSuchDistro\x")));
    }

    #[test]
    fn look_alike_hosts_are_not_wsl() {
        for path in [
            r"\\wsl\Ubuntu",
            r"\\wsl.localhost.example\share",
            r"\\mywsl$\share",
            r"\\server\wsl$\x",
            r"\\server\wsl.localhost",
            r"\\?\UNC\server\wsl.localhost\x",
            r"\\.\UNC\server\wsl$",
            r"\\?\Volume{0b1e2c3d-0000-0000-0000-100000000000}\wsl$",
            r"\\.\PhysicalDrive0",
            r"relative\wsl$\x",
            r"\wsl.localhost\x",
            "",
        ] {
            assert!(!on_wsl(Path::new(path), no_drive), "{path}");
        }
    }

    #[test]
    fn a_drive_counts_only_when_it_points_into_wsl() {
        let asked = std::cell::RefCell::new(Vec::new());
        let drives = |letter: u8| {
            asked.borrow_mut().push(letter);
            let target = match letter {
                b'C' => r"\Device\HarddiskVolume3",
                // SMB mapped drives, as `QueryDosDeviceW` gives them
                b'K' => r"\Device\LanmanRedirector\;K:00000000000421da\carbon\Shared",
                b'Z' => r"\Device\LanmanRedirector\;Z:00000000000421da\192.168.0.113\midia",
                b'W' => r"\Device\P9Rdr\;W:00000000000421da\wsl.localhost\Ubuntu-24.04",
                b'V' => r"\Device\Mup\;P9Rdr\;V:0000000000000001\wsl$\Ubuntu",
                b'S' => r"\??\UNC\wsl.localhost\Ubuntu\tmp",
                b'T' => r"\??\UNC\server\share",
                b'U' => r"\Device\LanmanRedirector\;U:0000000000000001\WSL$\Ubuntu",
                b'Q' => r"\Device\LanmanRedirector\;Q:0000000000000001\server\wsl.localhost",
                _ => return None,
            };
            Some(target.to_owned())
        };
        for (path, wsl) in [
            (r"C:\docs", false),
            (r"K:\projects", false),
            (r"z:\midia", false),
            (r"W:\home\edelm", true),
            (r"\\?\W:\home", true),
            (r"v:\x", true),
            (r"S:\", true),
            (r"T:\", false),
            (r"U:\", true),
            (r"Q:\", false),
            (r"G:\", false),
        ] {
            assert_eq!(on_wsl(Path::new(path), drives), wsl, "{path}");
        }
        assert!(asked.borrow().iter().all(u8::is_ascii_uppercase));
    }

    #[test]
    fn shares_never_ask_for_a_drive() {
        let never = |_: u8| -> Option<String> { panic!("no drive to ask") };
        assert!(on_wsl(Path::new(r"\\wsl$\Ubuntu"), never));
        assert!(!on_wsl(Path::new(r"\\server\share"), never));
        assert!(!on_wsl(Path::new(r"\\?\UNC\server\share"), never));
    }

    #[test]
    fn the_real_drive_lookup_reads_local_drives_as_local() {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        let letter = system.as_bytes()[0].to_ascii_uppercase();
        let target = drive_target(letter).expect("the system drive has a device");
        assert!(!target_is_wsl(&target), "{target}");
        assert!(!is_wsl(Path::new(&format!(r"{system}\"))));
    }
}
