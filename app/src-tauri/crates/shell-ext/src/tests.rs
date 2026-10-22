use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::core::{Interface, GUID};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, S_OK,
};
use windows::Win32::System::Com::{CoTaskMemFree, IClassFactory};
use windows::Win32::UI::Shell::{IExplorerCommand, ECS_ENABLED};

use super::*;

#[test]
fn labels_follow_the_language_not_the_region() {
    assert_eq!(
        labels::pick(&labels::OPEN, &["de-AT"]),
        "Mit PerMark öffnen"
    );
    assert_eq!(
        labels::pick(&labels::OPEN, &["sv-SE", "pt-BR"]),
        "Abrir com o PerMark"
    );
    assert_eq!(
        labels::pick(&labels::NEW_MARKDOWN, &["sv-SE", "pt-PT"]),
        "Novo ficheiro Markdown com o PerMark"
    );
    assert_eq!(
        labels::pick(&labels::NEW_MARKDOWN, &["pt"]),
        "Novo arquivo Markdown com o PerMark"
    );
    assert_eq!(
        labels::pick(&labels::NEW_MARKDOWN, &["zh-Hant-TW"]),
        "用 PerMark 新建 Markdown 文件"
    );
    assert_eq!(
        labels::pick(&labels::OPEN, &["sv-SE", "ja-JP"]),
        "PerMark で開く"
    );
    assert_eq!(
        labels::pick(&labels::NEW_MARKDOWN, &["JA_jp"]),
        "PerMark で新しい Markdown ファイルを作成"
    );
    assert_eq!(labels::pick(&labels::OPEN, &["ko-KR"]), "PerMark로 열기");
    assert_eq!(
        labels::pick(&labels::OPEN, &["fr-CA"]),
        "Ouvrir avec PerMark"
    );
    assert_eq!(
        labels::pick(&labels::OPEN, &["ca-ES-valencia"]),
        "Obre amb PerMark"
    );
    assert_eq!(labels::pick(&labels::OPEN, &["gl-ES"]), "Abrir con PerMark");
    assert_eq!(
        labels::pick(&labels::OPEN, &["ro-RO"]),
        "Deschide cu PerMark"
    );
    assert_eq!(labels::pick(&labels::OPEN, &["sv-SE"]), "Open with PerMark");
    assert_eq!(
        labels::pick::<&str>(&labels::OPEN, &[]),
        "Open with PerMark"
    );
    assert_eq!(
        labels::pick(&labels::OPEN, &["", "-", "_x"]),
        "Open with PerMark"
    );
}

#[test]
fn folders_are_absolute_unique_and_at_most_fifteen() {
    let picked = launch::folders_to_open(vec![
        PathBuf::from(r"C:\Work\Ünï Docs"),
        PathBuf::from(r"c:\work\ünï docs"),
        PathBuf::from("relative"),
        PathBuf::from("--flag"),
        PathBuf::from(r"D:\b"),
    ]);
    assert_eq!(
        picked,
        [PathBuf::from(r"C:\Work\Ünï Docs"), PathBuf::from(r"D:\b")]
    );
    let many: Vec<PathBuf> = (0..40)
        .map(|n| PathBuf::from(format!(r"C:\f{n}")))
        .collect();
    let picked = launch::folders_to_open(many);
    assert_eq!(picked.len(), launch::MOST_FOLDERS);
    assert_eq!(picked[0], PathBuf::from(r"C:\f0"));
}

#[test]
fn the_program_is_beside_the_dll_and_gets_one_argument() {
    let dll = Path::new(
        r"C:\Program Files\WindowsApps\EdelmarSchneider.TrailMark_0.6.0.0_x64__k\permark_shell.dll",
    );
    let program = launch::package_program(dll).unwrap();
    assert_eq!(program, dll.with_file_name("permark.exe"));
    let folder = Path::new(r"C:\My Docs\项目");
    let open = launch::command(&program, folder, false);
    let args: Vec<_> = open.get_args().collect();
    assert_eq!(args, [folder.as_os_str()]);
    assert_eq!(open.get_current_dir(), program.parent());
    let new = launch::command(&program, folder, true);
    let args: Vec<_> = new.get_args().collect();
    assert_eq!(
        args,
        [std::ffi::OsStr::new("--new-markdown"), folder.as_os_str()]
    );
}

#[test]
fn starting_nothing_or_a_missing_program_fails() {
    let error = launch::start(Path::new(r"C:\nowhere\permark.exe"), &[], false).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    let missing = std::env::temp_dir().join("permark-missing-program-xyz.exe");
    assert!(launch::start(&missing, &[PathBuf::from(r"C:\")], false).is_err());
}

fn factory_of(class: &GUID) -> Result<IClassFactory, windows::core::HRESULT> {
    let mut object: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid ids and a valid place for the answer.
    let code = unsafe { DllGetClassObject(class, &IClassFactory::IID, &raw mut object) };
    if code != S_OK {
        return Err(code);
    }
    // SAFETY: on success `object` holds an `IClassFactory` we now own.
    Ok(unsafe { IClassFactory::from_raw(object) })
}

#[test]
fn the_dll_answers_for_its_two_classes_only() {
    assert!(factory_of(&CLSID_OPEN_WITH_PERMARK).is_ok());
    assert!(factory_of(&CLSID_NEW_MARKDOWN).is_ok());
    assert_eq!(
        factory_of(&GUID::from_u128(0x1234)).err(),
        Some(CLASS_E_CLASSNOTAVAILABLE)
    );
    // SAFETY: null pointers are what is under test; the function checks them.
    let code =
        unsafe { DllGetClassObject(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
    assert_eq!(code, E_POINTER);
}

#[test]
fn the_command_says_what_it_is() {
    let factory = factory_of(&CLSID_NEW_MARKDOWN).unwrap();
    // SAFETY: plain COM calls on live interfaces; strings handed over are freed with CoTaskMemFree.
    unsafe {
        let command: IExplorerCommand = factory.CreateInstance(None).unwrap();
        let title = command.GetTitle(None).unwrap();
        let text = title.to_string().unwrap();
        CoTaskMemFree(Some(title.0.cast_const().cast()));
        assert!(labels::NEW_MARKDOWN.contains(&text.as_str()), "{text}");
        assert_eq!(command.GetCanonicalName().unwrap(), CLSID_NEW_MARKDOWN);
        assert_eq!(
            command.GetState(None, false).unwrap(),
            ECS_ENABLED.0.unsigned_abs()
        );
        let icon = command.GetIcon(None).unwrap();
        let icon_text = icon.to_string().unwrap();
        CoTaskMemFree(Some(icon.0.cast_const().cast()));
        assert!(icon_text.ends_with("permark.exe,0"), "{icon_text}");
        // never part of another object
        let outer: windows::core::IUnknown = factory.cast().unwrap();
        let aggregated: windows::core::Result<IExplorerCommand> = factory.CreateInstance(&outer);
        assert_eq!(aggregated.unwrap_err().code(), CLASS_E_NOAGGREGATION);
    }
}

#[test]
fn lock_server_never_goes_below_zero() {
    module::lock_server(false);
    module::lock_server(false);
    module::lock_server(true);
    module::lock_server(false);
    // other tests may hold objects at the same moment: only the floor is checked here
    module::lock_server(false);
}
