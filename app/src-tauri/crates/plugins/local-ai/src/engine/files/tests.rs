//! Worker files over scripted downloads (AC-6): pinned unit fetched + verified, hash mismatch refused, no pin ->
//! typed "not available yet".

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use permark_core::downloads::{
    DownloadError, DownloadKit, MemorySecrets, Scripted, ScriptedTransport, Update,
};
use sha2::{Digest, Sha256};

use super::*;

const EXE: &[u8] = b"pretend worker program";
const LIB: &[u8] = b"pretend ggml library bytes";

fn file(name: &'static str, body: &[u8]) -> WorkerFile {
    WorkerFile {
        name,
        path: Box::leak(format!("{RELEASES}/worker-v0.0.1/{name}").into_boxed_str()),
        bytes: body.len() as u64,
        sha256: Sha256::digest(body).into(),
    }
}

/// Pin of a test unit: worker + one library.
fn pin() -> &'static WorkerPin {
    let files: &'static [WorkerFile] =
        Box::leak(vec![file("worker.exe", EXE), file("ggml.dll", LIB)].into_boxed_slice());
    Box::leak(Box::new(WorkerPin {
        tag: "worker-v0.0.1",
        platform: "test-x86_64",
        variant: Variant::Vulkan,
        exe: "worker.exe",
        files,
    }))
}

fn url(file: &WorkerFile) -> String {
    format!("https://github.com{}", file.path)
}

fn kit(transport: ScriptedTransport) -> DownloadKit {
    DownloadKit {
        transport: Arc::new(transport),
        secrets: Arc::new(MemorySecrets::default()),
    }
}

/// Files over `store` + the updates they hear.
fn files_with(
    pin: Option<&'static WorkerPin>,
    transport: ScriptedTransport,
    store: PathBuf,
) -> (WorkerFiles, Receiver<Update>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let listener: Listener = Arc::new(move |_, update: &Update| {
        let _ = tx.lock().unwrap().send(update.clone());
    });
    (
        WorkerFiles::with_pin(pin, Some(&kit(transport)), store, listener),
        rx,
    )
}

/// Last update after the progress ones.
fn outcome(rx: &Receiver<Update>) -> Update {
    loop {
        match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            Update::Progress(_) => {}
            other => return other,
        }
    }
}

#[test]
fn every_pin_makes_a_unit_holding_its_worker() {
    for pin in PINS {
        assert!(unit_of(pin).is_ok(), "{pin:?}");
    }
    assert!(unit_of(pin()).is_ok());
    let no_exe = WorkerPin {
        exe: "missing.exe",
        ..*pin()
    };
    assert_eq!(unit_of(&no_exe).err(), Some(FilesError::BadPin));
}

#[test]
fn no_pin_for_this_computer_is_not_available_yet() {
    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join(DIR);
    let (files, _rx) = files_with(None, ScriptedTransport::new(), store.clone());
    assert_eq!(files.unit().err(), Some(FilesError::NotAvailable));
    assert_eq!(files.enqueue(), Err(FilesError::NotAvailable));
    assert!(!files.is_installed());
    assert_eq!(files.exe().unwrap_err().kind(), io::ErrorKind::NotFound);
    files.cancel();
    assert!(!store.exists(), "nothing created");
}

#[test]
fn no_download_service_refuses_the_fetch() {
    let tmp = tempfile::tempdir().unwrap();
    let files = WorkerFiles::with_pin(Some(pin()), None, tmp.path().join(DIR), Arc::new(|_, _| {}));
    assert_eq!(files.enqueue(), Err(FilesError::NoDownloads));
    assert!(!files.is_installed());
}

#[test]
fn the_pinned_unit_downloads_verified_and_resolves_the_worker() {
    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join(DIR);
    let pin = pin();
    let transport = ScriptedTransport::new()
        .answer(&url(&pin.files[0]), Scripted::ok(EXE))
        .answer(&url(&pin.files[1]), Scripted::ok(LIB));
    let (files, rx) = files_with(Some(pin), transport, store.clone());
    assert!(!files.is_installed());
    assert!(!store.exists(), "dormant until enqueue");
    files.enqueue().unwrap();
    assert!(matches!(outcome(&rx), Update::Done));
    assert!(files.is_installed());
    let exe = files.exe().unwrap();
    assert_eq!(
        exe,
        store
            .join("worker-v0.0.1-test-x86_64-vulkan")
            .join("worker.exe")
    );
    assert_eq!(std::fs::read(&exe).unwrap(), EXE);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "executable");
    }
    // files + install manifest
    let told = bytes(&store);
    let pinned = (EXE.len() + LIB.len()) as u64;
    assert!(told > pinned && told < pinned + 4096, "{told}");
    files.shutdown();
}

#[test]
fn a_file_of_another_hash_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join(DIR);
    let pin = pin();
    let mut forged = LIB.to_vec();
    forged[0] ^= 1;
    let transport = ScriptedTransport::new()
        .answer(&url(&pin.files[0]), Scripted::ok(EXE))
        .answer(&url(&pin.files[1]), Scripted::ok(&forged));
    let (files, rx) = files_with(Some(pin), transport, store);
    files.enqueue().unwrap();
    assert!(matches!(
        outcome(&rx),
        Update::Failed(DownloadError::Integrity)
    ));
    assert!(!files.is_installed());
    assert_eq!(files.exe().unwrap_err().kind(), io::ErrorKind::NotFound);
}

#[test]
fn bytes_of_a_missing_store_are_zero() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(bytes(&tmp.path().join(DIR)), 0);
}
