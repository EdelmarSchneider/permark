//! Which build of the program this is: the one the project publishes itself, or the one in the Microsoft Store.
//!
//! The two builds are the same code, told apart when the program is compiled (the cargo feature `store`, set together
//! with `tauri.msstore.conf.json` by the packaging script). What the Store build does differently:
//!
//! * **It does not update itself.** The Store updates the apps it installs, and a package cannot replace its own files.
//!   The updater plugin is not registered, and the interface leaves out "Check for updates".
//! * **An agent starts it by its alias.** The package puts `permark` on the `PATH` (an app execution alias). The
//!   program's own path is inside the package's folder, which changes with every version and is not meant to be run
//!   directly, so the setup lines an agent is given name the alias instead (see [`agent_program`]).
//! * **It writes no file manager entries.** The package declares its file association in its manifest, and what a
//!   packaged program writes to the registry goes to a private copy the shell never reads (see [`shell_integration`]).
//!   The folder "Open with PerMark" menu is the package's too: its manifest names a COM component, the DLL built from
//!   `shell-ext/` and packed beside the program, which Windows loads when the menu is drawn. The program has nothing to
//!   do with it, and Settings says so.
//!
//! [`shell_integration`]: crate::shell_integration

use std::path::PathBuf;

use serde::Serialize;

/// Where the running program comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Distribution {
    /// Installed from the project's own installers (and run from a development checkout).
    Standard,
    /// Installed from the Microsoft Store.
    Store,
}

impl Distribution {
    pub const fn is_store(self) -> bool {
        matches!(self, Self::Store)
    }
}

/// The build that is running.
pub const fn current() -> Distribution {
    if cfg!(feature = "store") {
        Distribution::Store
    } else {
        Distribution::Standard
    }
}

/// The name of the program's app execution alias: what the package puts on the `PATH` (`AppxManifest.xml`).
const STORE_ALIAS: &str = "permark";

/// The program an agent should start (`<program> mcp`), as the setup lines name it: the alias in the Store build, where
/// the program's own path is not meant to be run, and the path of the running program (`running`) anywhere else.
pub(crate) fn agent_program(
    distribution: Distribution,
    running: Option<PathBuf>,
) -> Option<String> {
    if distribution.is_store() {
        Some(STORE_ALIAS.to_owned())
    } else {
        running.map(|path| path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_follows_the_cargo_feature() {
        assert_eq!(current().is_store(), cfg!(feature = "store"));
        assert_eq!(
            current() == Distribution::Standard,
            !cfg!(feature = "store")
        );
    }

    #[test]
    fn the_interface_is_told_the_build_by_name() {
        let names = |distribution| serde_json::to_value(distribution).expect("serializes");
        assert_eq!(names(Distribution::Standard), "standard");
        assert_eq!(names(Distribution::Store), "store");
    }

    #[test]
    fn the_store_build_names_its_alias_whatever_its_own_path_is() {
        let packaged = PathBuf::from(
            r"C:\Program Files\WindowsApps\EdelmarSchneider.PerMark_0.4.0.0_x64__abc123\permark.exe",
        );
        assert_eq!(
            agent_program(Distribution::Store, Some(packaged)).as_deref(),
            Some("permark")
        );
        // Even when the program cannot tell where it is.
        assert_eq!(
            agent_program(Distribution::Store, None).as_deref(),
            Some("permark")
        );
    }

    /// The setup lines name an alias; the package has to declare that alias, for the program it holds.
    #[test]
    fn the_package_declares_the_alias_the_setup_lines_name() {
        const MANIFEST: &str = include_str!("../msstore/AppxManifest.xml");
        let program = format!("{}.exe", env!("CARGO_PKG_NAME"));
        assert_eq!(program, format!("{STORE_ALIAS}.exe"));
        assert!(MANIFEST.contains(&format!(r#"Alias="{program}""#)));
        assert!(MANIFEST.contains(&format!(r#"Executable="{program}""#)));
        // a file association hands the file's path over as the first argument, as `permark <path>` takes it
        assert!(MANIFEST.contains(r#"Parameters="&quot;%1&quot;""#));
        // the tokens the packaging script fills in
        assert!(MANIFEST.contains(r#"Version="@VERSION@""#));
        assert!(MANIFEST.contains(r#"Description="@DESCRIPTION@""#));
    }

    /// The folder menu entry is made of four things that name each other: the manifest, the DLL's crate, the packaging script
    /// and the DLL's own class id. Nothing here installs a package, so this is what keeps them from drifting apart.
    #[test]
    fn the_package_declares_the_folder_menu_entry_for_the_dll_it_packs() {
        const MANIFEST: &str = include_str!("../msstore/AppxManifest.xml");
        const SHELL_CRATE: &str = include_str!("../shell-ext/Cargo.toml");
        const SHELL_SOURCE: &str = include_str!("../shell-ext/src/lib.rs");
        const SCRIPT: &str = include_str!("../../scripts/build-msix.ps1");

        // the crate builds a DLL of the name the manifest and the script use
        let library = r#"name = "permark_shell""#;
        assert!(SHELL_CRATE.contains(library));
        assert!(SHELL_CRATE.contains(r#"crate-type = ["cdylib""#));
        assert!(MANIFEST.contains(r#"Path="permark_shell.dll""#));
        assert!(SCRIPT.contains("'permark_shell.dll'"));

        // one class id: declared as a COM class, and registered for folders and for the background of a folder
        let id = "D5FFB935-7A42-4E3D-90E1-0180A7999B94";
        assert!(MANIFEST.contains(&format!(r#"<com:Class Id="{id}""#)));
        assert_eq!(MANIFEST.matches(&format!(r#"Clsid="{id}""#)).count(), 2);
        assert!(MANIFEST.contains(r#"<desktop5:ItemType Type="Directory">"#));
        assert!(MANIFEST.contains(r#"<desktop5:ItemType Type="Directory\Background">"#));
        assert!(MANIFEST.contains(r#"Category="windows.fileExplorerContextMenus""#));
        // the DLL is loaded by a helper process, not by Explorer
        assert!(MANIFEST.contains("<com:SurrogateServer"));
        // the same id as the one the DLL answers to (its own test checks the number, this the text)
        assert!(SHELL_SOURCE.contains("0xD5FF_B935_7A42_4E3D_90E1_0180_A799_9B94"));
        // the program the DLL starts is the one the manifest declares
        assert!(MANIFEST.contains(r#"Executable="permark.exe""#));
    }

    /// The Store build is configured without the updater: the overlay the packaging script builds with removes its
    /// settings and the signed update files that go with them.
    #[test]
    fn the_store_overlay_removes_the_updater() {
        const OVERLAY: &str = include_str!("../tauri.msstore.conf.json");
        let overlay: serde_json::Value = serde_json::from_str(OVERLAY).expect("valid json");
        assert!(overlay["plugins"]["updater"].is_null());
        assert!(overlay["plugins"]
            .as_object()
            .is_some_and(|plugins| plugins.contains_key("updater")));
        assert_eq!(overlay["bundle"]["createUpdaterArtifacts"], false);
    }

    #[test]
    fn the_standard_build_names_its_own_path() {
        let path = PathBuf::from("/opt/permark/permark");
        assert_eq!(
            agent_program(Distribution::Standard, Some(path.clone())),
            Some(path.display().to_string())
        );
        assert_eq!(agent_program(Distribution::Standard, None), None);
    }
}
