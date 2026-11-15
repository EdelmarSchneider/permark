use super::*;

#[test]
fn page_windows_are_told_apart_from_the_persons_windows() {
    assert!(is_sheet("export-1"));
    assert!(!is_sheet("main"));
    assert!(!is_sheet("w-2"));
}

#[test]
fn a_page_window_goes_to_its_page_only() {
    let page = page_url().unwrap();
    assert!(is_page_url(&page));
    let other = |text: &str| !is_page_url(&Url::parse(text).unwrap());
    assert!(other("https://example.com/"));
    assert!(other(&format!("{page}other")));
    assert!(other("http://tauri.localhost/"));
    assert!(other("data:text/html,<p>x</p>"));
}

#[test]
fn each_page_is_served_to_its_own_window_until_forgotten() {
    let pages = Pages::default();
    let a = pages.add("<p>a</p>".to_owned());
    let b = pages.add("<p>b</p>".to_owned());
    assert_ne!(a, b);
    assert!(is_sheet(&a) && is_sheet(&b));
    assert_eq!(
        pages.page(&a).as_deref().map(String::as_str),
        Some("<p>a</p>")
    );
    pages.remove(&a);
    assert!(pages.page(&a).is_none());
    assert!(pages.page("main").is_none());
    assert!(pages.page(&b).is_some());
}

/// A 1 x 1 PNG, drawn large.
#[cfg(all(windows, feature = "webview-tests"))]
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

/// Three A5 landscape pages: a picture, an inline SVG, a page counter in the bottom margin.
#[cfg(all(windows, feature = "webview-tests"))]
fn sample() -> String {
    let filler = "<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore \
                  et dolore magna aliqua.</p>"
        .repeat(4);
    format!(
        "<!doctype html><html><head><meta charset=utf-8><meta name=color-scheme content=light><style>\
         @page {{ size: A5 landscape; margin: 15mm; \
         @bottom-center {{ content: counter(page) \" / \" counter(pages); font: 9pt sans-serif; }} }}\
         body {{ font: 11pt Georgia, serif; background: #fffdf5; }} h1 {{ break-before: page; }} \
         h1:first-of-type {{ break-before: auto; }} img {{ width: 30mm; image-rendering: pixelated; }}\
         </style></head><body>\
         <h1>One</h1><img alt=dot src=\"data:image/png;base64,{PNG}\">{filler}\
         <h1>Two</h1><svg width=200 height=80 viewBox=\"0 0 200 80\"><rect width=200 height=80 rx=12 fill=#2b6cb0 />\
         <text x=20 y=50 fill=white font-size=28>inline SVG</text></svg>{filler}\
         <h1>Three</h1>{filler}</body></html>"
    )
}

/// Visible top-level windows on the desktop now: handle, class.
#[cfg(all(windows, feature = "webview-tests"))]
#[allow(unsafe_code)] // Win32 window enumeration (test only)
fn top_windows() -> Vec<(isize, String)> {
    use ::windows::core::BOOL;
    use ::windows::Win32::Foundation::{HWND, LPARAM};
    use ::windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, IsWindowVisible};

    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut class = [0_u16; 64];
            let len = usize::try_from(unsafe { GetClassNameW(hwnd, &mut class) }).unwrap_or(0);
            let found = unsafe { &mut *(found.0 as *mut Vec<(isize, String)>) };
            found.push((hwnd.0 as isize, String::from_utf16_lossy(&class[..len])));
        }
        BOOL(1)
    }
    let mut found: Vec<(isize, String)> = Vec::new();
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&raw mut found as isize)) };
    found
}

/// Waits for a print dialog that was not there `before` and closes it with its Cancel button: the Windows 11 dialog
/// (`ApplicationFrameWindow`, shown by the shell) only through UI Automation, the classic one (`#32770`) by
/// `WM_CLOSE`. Whether one showed.
#[cfg(all(windows, feature = "webview-tests"))]
#[allow(unsafe_code)] // Win32 message to a window (test only)
fn cancel_print_dialog(before: &[(isize, String)]) -> bool {
    use ::windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use ::windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        let dialog = top_windows().into_iter().find(|(hwnd, class)| {
            !before.iter().any(|b| b.0 == *hwnd)
                && (class == "ApplicationFrameWindow" || class == "#32770")
        });
        if let Some((hwnd, class)) = dialog {
            std::thread::sleep(Duration::from_millis(1500));
            if class == "#32770" {
                let _ = unsafe {
                    PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0))
                };
            } else {
                let script = format!(
                    "Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes;                      $w = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]{hwnd});                      $c = New-Object System.Windows.Automation.PropertyCondition(                     [System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'CloseButton');                      $w.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $c).GetCurrentPattern(                     [System.Windows.Automation.InvokePattern]::Pattern).Invoke()"
                );
                let _ = std::process::Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .status();
            }
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// The real web view (one app per process: both steps in one test).
/// - PDF: a hidden page window renders three A5 landscape pages with a picture, an SVG and page numbers.
/// - Print: the system print dialog shows for a hidden page window; closing it ends the print call.
///
/// Run (from `app/src-tauri`): `cargo test --lib --features webview-tests print::tests -- --ignored --nocapture`;
/// `PERMARK_PRINT_OUT=<file.pdf>` keeps the PDF to look at. Puts a print dialog on screen for a moment.
#[cfg(all(windows, feature = "webview-tests"))]
#[test]
#[ignore = "opens a real system web view and the print dialog"]
fn a_hidden_page_window_renders_a_paged_pdf_and_prints() {
    use std::time::Instant;

    use permark_core::plugins::{Margins, Orientation, PageSize};
    use tauri::RunEvent;

    let data = tempfile::tempdir().unwrap();
    let data_dir = data.path().join("webview");
    let app = tauri::Builder::default()
        .any_thread()
        .manage(Pages::default())
        .register_asynchronous_uri_scheme_protocol(SCHEME, handle)
        .build(tauri::generate_context!())
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = app.handle().clone();
    std::thread::spawn(move || {
        let setup = PageSetup {
            size: PageSize::A5,
            orientation: Orientation::Landscape,
            margins: Margins::Narrow,
            page_numbers: true,
        };
        let started = Instant::now();
        let sheet = Sheet::open(&worker, sample(), Some(data_dir.clone()));
        let loaded = started.elapsed();
        let bytes = sheet.and_then(|sheet| native::pdf(&sheet, setup));
        let pdf = (bytes, loaded, started.elapsed());
        // print: the dialog the page window opens is closed again from here
        let before = top_windows();
        let closer = std::thread::spawn(move || cancel_print_dialog(&before));
        let started = Instant::now();
        let printed = Sheet::open(&worker, sample(), Some(data_dir))
            .and_then(|sheet| native::print(&sheet, setup));
        let shown = closer.join().unwrap_or(false);
        let _ = tx.send((pdf, printed, shown, started.elapsed()));
        worker.exit(0);
    });
    app.run_return(|_, event| {
        // a page window closing is not the end of the run: the worker ends it
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    let ((bytes, loaded, total), printed, shown, print_time) = rx.recv().unwrap();
    // the web view's browser process lets go of its data folder a moment after the app ends
    for _ in 0..50 {
        if std::fs::remove_dir_all(data.path()).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let bytes = bytes.unwrap();
    println!(
        "loaded in {loaded:?}, PDF of {} bytes in {total:?}; print dialog shown: {shown}, print call {print_time:?}",
        bytes.len()
    );
    if let Some(out) = std::env::var_os("PERMARK_PRINT_OUT") {
        std::fs::write(out, &bytes).unwrap();
    }
    assert!(bytes.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&bytes);
    let pages = text.matches("/Type /Page").count() - text.matches("/Type /Pages").count();
    assert_eq!(pages, 3, "three pages");
    // A5 landscape: 595 x 420 points
    assert_eq!(
        text.matches("/MediaBox [0 0 594.95996 420]").count(),
        3,
        "A5 landscape pages"
    );
    assert!(shown, "the print dialog showed");
    printed.unwrap();
}
