//! `WKWebView`: PDF through the web view's own `AppKit` print operation saving to a file in a private temp folder (read
//! back, then removed); printing through the same operation with the print panel. The operation runs modal to the
//! page's window (run on its own it gives blank pages: the web view draws its pages while the run loop turns). No page
//! numbers: `WebKit` draws no `@page` margin boxes.
// AppKit | WebKit through objc2: messages to Objective-C objects are `unsafe`; each runs on the main thread inside
// `with_webview`, on the objects the web view handed out.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSPaperOrientation, NSPrintInfo, NSPrintJobSavingURL, NSPrintOperation, NSPrintSaveJob,
};
use objc2_foundation::{NSError, NSNumber, NSSize, NSString, NSURL};
use objc2_web_kit::WKWebView;
use permark_core::plugins::{Orientation, PageSetup, PrintCapabilities, PrintError};
use tauri::webview::PlatformWebview;

use super::{
    failed, on_webview, until, wait, Answer, Sheet, PRINT_TIMEOUT, RENDER_TIMEOUT, SETTLED,
    SETTLE_TIMEOUT,
};

pub const CAPABILITIES: PrintCapabilities = PrintCapabilities {
    pdf: true,
    print: true,
    page_numbers: false,
    native_margins: true,
    snap_needs_cups: false,
};

pub const fn capabilities() -> PrintCapabilities {
    CAPABILITIES
}

const POINTS_PER_MM: f64 = 72.0 / 25.4;

type Sender<T> = mpsc::Sender<Answer<T>>;

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements; `PrintEnd` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PerMarkPrintEnd"]
    #[ivars = Sender<bool>]
    struct PrintEnd;

    impl PrintEnd {
        /// The `didRunSelector` of a modal print operation (`success`: no = failed | cancelled). `context`: the
        /// reference to this object handed over when the operation started (an operation does not keep its delegate).
        #[unsafe(method(printOperationDidRun:success:contextInfo:))]
        fn did_run(&self, _operation: &NSPrintOperation, success: Bool, context: *mut c_void) {
            let _ = self.ivars().send(Ok(success.as_bool()));
            // released when this turn of the run loop is over, not while its method still runs
            if let Some(held) = unsafe { Retained::from_raw(context.cast::<Self>()) } {
                let _ = Retained::autorelease_ptr(held);
            }
        }
    }

    unsafe impl NSObjectProtocol for PrintEnd {}
);

impl PrintEnd {
    fn new(mtm: MainThreadMarker, tx: Sender<bool>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(tx);
        unsafe { msg_send![super(this), init] }
    }
}

fn web_view(webview: &PlatformWebview) -> Result<Retained<WKWebView>, String> {
    unsafe { Retained::retain(webview.inner().cast::<WKWebView>()) }
        .ok_or_else(|| "no web view".to_owned())
}

/// Whether `script`, run in the page, comes out true (a page that allows no content script still runs the host's).
fn evaluate(sheet: &Sheet, script: &'static str) -> Result<bool, PrintError> {
    let rx = on_webview(sheet, move |webview, tx| {
        let view = web_view(webview)?;
        let done = RcBlock::new(move |result: *mut AnyObject, _error: *mut NSError| {
            // a boolean comes back as a number
            let yes = unsafe { result.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSNumber>)
                .is_some_and(NSNumber::boolValue);
            let _ = tx.send(Ok(yes));
        });
        unsafe {
            view.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&done));
        };
        Ok::<(), String>(())
    })?;
    wait(&rx, SETTLE_TIMEOUT)
}

/// The loaded page's fonts and pictures are ready.
pub(super) fn settle(sheet: &Sheet) -> Result<(), PrintError> {
    until(SETTLE_TIMEOUT, || evaluate(sheet, SETTLED))
}

/// Starts the web view's print operation on `setup`'s paper, modal to its window: saved to `file` without any panel,
/// else through the print panel. Whether it succeeded comes through `tx`.
fn run(
    webview: &PlatformWebview,
    setup: PageSetup,
    file: Option<&Path>,
    tx: Sender<bool>,
) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or_else(|| "not on the main thread".to_owned())?;
    let view = web_view(webview)?;
    let window = view
        .window()
        .ok_or_else(|| "the web view has no window".to_owned())?;
    let page = setup.page();
    let info = NSPrintInfo::new();
    // orientation first: the paper size set after it is the laid-out one either way
    info.setOrientation(match setup.orientation {
        Orientation::Portrait => NSPaperOrientation::Portrait,
        Orientation::Landscape => NSPaperOrientation::Landscape,
    });
    info.setPaperSize(NSSize::new(
        page.width_mm * POINTS_PER_MM,
        page.height_mm * POINTS_PER_MM,
    ));
    // `WebKit` lays the page out inside the print info's margins, not those of the page's `@page` rule
    let margin = page.margin_mm * POINTS_PER_MM;
    info.setTopMargin(margin);
    info.setBottomMargin(margin);
    info.setLeftMargin(margin);
    info.setRightMargin(margin);
    if let Some(file) = file {
        let url = NSURL::from_file_path(file).ok_or_else(|| "no file URL".to_owned())?;
        unsafe {
            info.setJobDisposition(NSPrintSaveJob);
            info.dictionary().insert(NSPrintJobSavingURL, &url);
        }
    }
    let operation = unsafe { view.printOperationWithPrintInfo(&info) };
    operation.setShowsPrintPanel(file.is_none());
    operation.setShowsProgressPanel(file.is_none());
    let end = PrintEnd::new(mtm, tx);
    let context = Retained::into_raw(end.clone()).cast::<c_void>();
    unsafe {
        operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
            &window,
            Some(&end),
            Some(sel!(printOperationDidRun:success:contextInfo:)),
            context,
        );
    }
    Ok(())
}

/// The page as a PDF of `setup`'s paper.
pub(super) fn pdf(sheet: &Sheet, setup: PageSetup) -> Result<Vec<u8>, PrintError> {
    // a folder only this user can enter
    let folder = tempfile::Builder::new()
        .prefix("permark-export-")
        .tempdir()
        .map_err(failed)?;
    let file: PathBuf = folder.path().join("page.pdf");
    let target = file.clone();
    let rx = on_webview(sheet, move |webview, tx| {
        run(webview, setup, Some(&target), tx)
    })?;
    if !wait(&rx, RENDER_TIMEOUT)? {
        return Err(failed("the print operation did not finish"));
    }
    std::fs::read(&file).map_err(failed)
}

/// The print panel for the page, its paper set to `setup`; returns once the job is handed over or the panel was
/// cancelled. The page's window is shown first: the panel is a sheet on it (not on `_asking`).
pub(super) fn print(
    sheet: &Sheet,
    setup: PageSetup,
    _asking: Option<&tauri::Window>,
) -> Result<(), PrintError> {
    let window = sheet.window()?;
    window.center().map_err(failed)?;
    window.show().map_err(failed)?;
    let rx = on_webview(sheet, move |webview, tx| run(webview, setup, None, tx))?;
    // printed | cancelled: both end the call
    wait(&rx, PRINT_TIMEOUT).map(|_printed| ())
}
