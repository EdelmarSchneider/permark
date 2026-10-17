//! The built DLL, as Windows would use it, minus Explorer and the registry: it is loaded with `LoadLibrary`, its two
//! exports are looked up with `GetProcAddress`, and the command is made through them and called through its interfaces.
//! Nothing is registered and no package is installed. Each test loads a copy of the DLL of its own, in a folder of its own,
//! so the copies keep their own counts of what is in use, and the program the command starts is the one beside the copy.
#![cfg(windows)]
#![allow(unsafe_code, clippy::too_many_lines)]

use std::ffi::c_void;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::sync::OnceLock;

use windows::core::{Interface, GUID, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::{
    FreeLibrary, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, HMODULE,
    S_FALSE, S_OK,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, IClassFactory, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Ole::IObjectWithSite;
use windows::Win32::UI::Shell::{
    IExplorerCommand, IShellItem, IShellItemArray, SHCreateItemFromParsingName,
    SHCreateShellItemArrayFromShellItem,
};

const CLSID: GUID = GUID::from_u128(0xD5FF_B935_7A42_4E3D_90E1_0180_A799_9B94);

type GetClassObject =
    unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
type CanUnloadNow = unsafe extern "system" fn() -> HRESULT;

/// The DLL, built by cargo next to the test programs. `cargo test` builds the library for the tests but not as a DLL, so the
/// test asks cargo for the DLL (once; it is up to date at once when it was built already).
fn built_dll() -> PathBuf {
    static DLL: OnceLock<PathBuf> = OnceLock::new();
    DLL.get_or_init(|| {
        let mut build = Command::new(env!("CARGO"));
        build
            .args(["build", "--lib", "--manifest-path"])
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        if !cfg!(debug_assertions) {
            build.arg("--release");
        }
        let status = build.status().expect("run cargo build");
        assert!(status.success(), "cargo build of the DLL failed");

        let tests = std::env::current_exe().expect("the test program");
        let profile = tests
            .parent()
            .and_then(Path::parent)
            .expect("target/<profile>/deps");
        let dll = profile.join("permark_shell.dll");
        assert!(dll.exists(), "{} was not built", dll.display());
        dll
    })
    .clone()
}

/// A copy of the DLL, loaded, in a folder of its own.
struct Loaded {
    folder: PathBuf,
    module: HMODULE,
    get_class_object: GetClassObject,
    can_unload_now: CanUnloadNow,
}

impl Loaded {
    /// `with_program`: a stand-in for `permark.exe` beside the DLL, as in the package. It is `where.exe`, which starts,
    /// finds nothing to look for and ends, without a window.
    fn new(name: &str, with_program: bool) -> Self {
        let folder =
            std::env::temp_dir().join(format!("permark-shell-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder for the copy");
        let dll = folder.join("permark_shell.dll");
        fs::copy(built_dll(), &dll).expect("copy the DLL");
        if with_program {
            let system = std::env::var_os("SystemRoot").expect("SystemRoot");
            fs::copy(
                Path::new(&system).join("System32").join("where.exe"),
                folder.join("permark.exe"),
            )
            .expect("copy the stand-in program");
        }

        let wide: Vec<u16> = dll.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a null-terminated path to a DLL that was just copied there.
        let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }.expect("LoadLibrary");
        // SAFETY: the names are the DLL's exports, whose signatures these types are (the same ones COM defines). The
        // module is freed only when the `Loaded` is dropped, after everything made from it.
        let (get_class_object, can_unload_now) = unsafe {
            let get = GetProcAddress(module, PCSTR(c"DllGetClassObject".as_ptr().cast()))
                .expect("DllGetClassObject is exported");
            let can = GetProcAddress(module, PCSTR(c"DllCanUnloadNow".as_ptr().cast()))
                .expect("DllCanUnloadNow is exported");
            (
                std::mem::transmute::<unsafe extern "system" fn() -> isize, GetClassObject>(get),
                std::mem::transmute::<unsafe extern "system" fn() -> isize, CanUnloadNow>(can),
            )
        };
        Self {
            folder,
            module,
            get_class_object,
            can_unload_now,
        }
    }

    fn factory(&self) -> IClassFactory {
        let mut factory = ptr::null_mut();
        // SAFETY: valid ids and a place for the answer.
        let code =
            unsafe { (self.get_class_object)(&CLSID, &IClassFactory::IID, &raw mut factory) };
        assert_eq!(code, S_OK);
        // SAFETY: the DLL wrote an owned IClassFactory.
        unsafe { IClassFactory::from_raw(factory) }
    }

    fn command(&self) -> IExplorerCommand {
        // SAFETY: a plain call through the factory.
        unsafe { self.factory().CreateInstance(None) }.expect("the command")
    }

    fn can_unload(&self) -> HRESULT {
        // SAFETY: a call to the export, which takes nothing.
        unsafe { (self.can_unload_now)() }
    }

    /// Whether the DLL exports `name`.
    fn exports(&self, name: &std::ffi::CStr) -> bool {
        // SAFETY: looking up a name that is not there is allowed and answers `None`.
        unsafe { GetProcAddress(self.module, PCSTR(name.as_ptr().cast())) }.is_some()
    }
}

impl Drop for Loaded {
    fn drop(&mut self) {
        // Everything made from the DLL is gone by now (the tests declare it after the `Loaded`), so it can be unloaded and
        // its copy deleted. The stand-in program that `Invoke` started cannot be waited for (the test has no handle to it),
        // and Windows will not delete the file of a running program, so the folder is tried until the program has ended,
        // for as long as a slow computer (a virus scanner holding the file, say) can take. Past that it is a failure of
        // the test, and not a folder left behind.
        // SAFETY: the module was loaded by `new`, once.
        unsafe {
            let _ = FreeLibrary(self.module);
        }
        let mut last = None;
        for _ in 0..600 {
            match fs::remove_dir_all(&self.folder) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(error) => last = Some(error),
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // A test that failed already is not made to panic a second time (that would abort the whole test program).
        if std::thread::panicking() {
            return;
        }
        panic!(
            "{} could not be removed after 30 s: {}",
            self.folder.display(),
            last.map_or_else(String::new, |error| error.to_string())
        );
    }
}

/// A string the command allocated, as text; freed.
fn text_of(string: windows::core::PWSTR) -> String {
    // SAFETY: the command hands over a null-terminated string allocated for the caller to free.
    unsafe {
        let text = string.to_string().expect("valid text");
        CoTaskMemFree(Some(string.0.cast_const().cast()));
        text
    }
}

fn folder_item(path: &Path) -> IShellItem {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a null-terminated path.
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.expect("a shell item")
}

fn selection(folders: &[&Path]) -> IShellItemArray {
    assert_eq!(folders.len(), 1, "one folder is enough for these tests");
    // SAFETY: a plain call on a live item.
    unsafe { SHCreateShellItemArrayFromShellItem(&folder_item(folders[0])) }.expect("an array")
}

#[test]
fn the_dll_exports_what_com_asks_of_a_server_and_nothing_to_register() {
    let loaded = Loaded::new("exports", false);
    assert!(loaded.exports(c"DllGetClassObject"));
    assert!(loaded.exports(c"DllCanUnloadNow"));
    // A package needs no registration code: Windows reads the manifest.
    assert!(!loaded.exports(c"DllRegisterServer"));
    assert!(!loaded.exports(c"DllUnregisterServer"));
}

#[test]
fn the_factory_is_made_for_its_class_and_for_no_other() {
    let loaded = Loaded::new("factory", false);
    let mut object = ptr::null_mut();
    let other = GUID::from_u128(0x0BAD_CAFE);
    // SAFETY: valid ids and a place for the answer.
    let code = unsafe {
        (loaded.get_class_object)(&raw const other, &IClassFactory::IID, &raw mut object)
    };
    assert_eq!(code, CLASS_E_CLASSNOTAVAILABLE);
    assert!(object.is_null());

    // an interface the factory does not have is refused and leaves nothing behind
    // SAFETY: as above.
    let code = unsafe { (loaded.get_class_object)(&CLSID, &IShellItem::IID, &raw mut object) };
    assert!(code.is_err());
    assert!(object.is_null());
    assert_eq!(loaded.can_unload(), S_OK);
}

#[test]
fn the_command_is_not_part_of_another_object() {
    let loaded = Loaded::new("aggregation", false);
    let factory = loaded.factory();
    let outer: windows::core::IUnknown = factory.clone().into();
    // SAFETY: a plain call.
    let result = unsafe { factory.CreateInstance::<_, IExplorerCommand>(&outer) };
    assert_eq!(result.expect_err("refused").code(), CLASS_E_NOAGGREGATION);
}

#[test]
fn the_command_says_what_it_is_when_explorer_asks() {
    let loaded = Loaded::new("asks", false);
    let command = loaded.command();
    let none = None::<&IShellItemArray>;
    // SAFETY: plain calls on a live command.
    unsafe {
        // the title is the text of one of the six languages the app has
        let title = text_of(command.GetTitle(none).expect("a title"));
        assert!(
            [
                "Open with PerMark",
                "Abrir com o PerMark",
                "Abrir con PerMark",
                "Mit PerMark öffnen",
                "用 PerMark 打开",
                "Apri con PerMark"
            ]
            .contains(&title.as_str()),
            "{title}"
        );
        // the icon is the first icon of the program beside the DLL
        let icon = text_of(command.GetIcon(none).expect("an icon"));
        assert_eq!(
            icon,
            format!("{},0", loaded.folder.join("permark.exe").display())
        );
        // shown and enabled (ECS_ENABLED is 0), also for the background of a folder, which has no selection
        assert_eq!(command.GetState(none, false).expect("a state"), 0);
        assert_eq!(command.GetFlags().expect("flags"), 0);
        assert_eq!(command.GetCanonicalName().expect("a name"), CLSID);
        assert_eq!(
            command.GetToolTip(none).expect_err("none").code(),
            E_NOTIMPL
        );
        assert_eq!(
            command.EnumSubCommands().expect_err("none").code(),
            E_NOTIMPL
        );
    }
}

#[test]
fn the_command_takes_the_site_explorer_gives_it() {
    let loaded = Loaded::new("site", false);
    let command = loaded.command();
    let with_site: IObjectWithSite = command.cast().expect("IObjectWithSite");
    let site: windows::core::IUnknown = loaded.factory().into();
    // SAFETY: plain calls on live interfaces.
    unsafe {
        assert_eq!(
            with_site
                .GetSite::<windows::core::IUnknown>()
                .expect_err("none yet")
                .code(),
            E_FAIL
        );
        with_site.SetSite(&site).expect("set");
        let got = with_site
            .GetSite::<windows::core::IUnknown>()
            .expect("the site");
        // the same object: COM compares objects by their IUnknown
        let same: windows::core::IUnknown = site.cast().expect("IUnknown");
        assert_eq!(got.as_raw(), same.as_raw());
        with_site
            .SetSite(None::<&windows::core::IUnknown>)
            .expect("cleared");
        assert!(with_site.GetSite::<windows::core::IUnknown>().is_err());
    }
}

#[test]
fn invoked_on_a_folder_it_starts_the_program_beside_the_dll() {
    let loaded = Loaded::new("invoke", true);
    // SAFETY: plain calls; COM is started for the shell items.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let command = loaded.command();
        let items = selection(&[loaded.folder.as_path()]);
        assert_eq!(command.GetState(&items, false).expect("a state"), 0);
        command
            .Invoke(&items, None)
            .expect("the program is started for the folder");
    }
}

#[test]
fn invoked_with_no_program_or_no_folder_it_fails_quietly() {
    // A package without its program (not a package Windows would install), and a menu on nothing: an error code each time,
    // nothing started, nothing crashed.
    let without_program = Loaded::new("no-program", false);
    // SAFETY: plain calls; COM is started for the shell items.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let items = selection(&[without_program.folder.as_path()]);
        let error = without_program
            .command()
            .Invoke(&items, None)
            .expect_err("no program");
        assert!(error.code().is_err());
    }

    let with_program = Loaded::new("no-folder", true);
    // the background of a folder gives no selection, and no site was given: there is no folder to open
    // SAFETY: a plain call.
    let error = unsafe {
        with_program
            .command()
            .Invoke(None::<&IShellItemArray>, None)
    }
    .expect_err("no folder");
    assert!(error.code().is_err());
}

#[test]
fn the_dll_can_be_unloaded_only_when_nothing_of_it_is_in_use() {
    let loaded = Loaded::new("unload", false);
    assert_eq!(loaded.can_unload(), S_OK);
    let factory = loaded.factory();
    assert_eq!(loaded.can_unload(), S_FALSE);
    let command: IExplorerCommand = {
        // SAFETY: a plain call.
        unsafe { factory.CreateInstance(None) }.expect("the command")
    };
    drop(factory);
    assert_eq!(loaded.can_unload(), S_FALSE, "the command is alive");
    drop(command);
    assert_eq!(loaded.can_unload(), S_OK);

    // a client can also ask to keep the DLL loaded
    let factory = loaded.factory();
    // SAFETY: plain calls.
    unsafe {
        factory.LockServer(true).expect("lock");
        drop(factory);
        assert_eq!(loaded.can_unload(), S_FALSE, "locked");
        loaded.factory().LockServer(false).expect("unlock");
    }
    assert_eq!(loaded.can_unload(), S_OK);
}
