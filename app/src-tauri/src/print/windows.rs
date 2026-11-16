//! `WebView2`: PDF through `PrintToPdfStream` (page size, margins, backgrounds, no browser header | footer), printing
//! through the system print dialog. Page numbers come from the page's own CSS (`@page` margin boxes, Chromium 131+).
// WebView2 is a COM API: every call into it is `unsafe` (no safe wrapper exists); each call runs on the UI thread
// inside `with_webview`, on interfaces the web view handed out.
#![allow(unsafe_code)]

use std::time::Duration;

use permark_core::plugins::{PageSetup, PrintCapabilities, PrintError};
use tauri::webview::PlatformWebview;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment6, ICoreWebView2_16,
    COREWEBVIEW2_PRINT_DIALOG_KIND_SYSTEM, COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT,
};
use webview2_com::{ExecuteScriptCompletedHandler, PrintToPdfStreamCompletedHandler};
use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::IStream;

use super::{
    on_webview, until, wait, Sheet, PRINT_TIMEOUT, RENDER_TIMEOUT, SETTLED, SETTLE_TIMEOUT,
};

pub const CAPABILITIES: PrintCapabilities = PrintCapabilities {
    pdf: true,
    print: true,
    page_numbers: true,
    native_margins: false,
    snap_needs_cups: false,
};

pub const fn capabilities() -> PrintCapabilities {
    CAPABILITIES
}

/// After the dialog closed: time for the web view to hand the job to the spooler before its window goes.
const SPOOL_GRACE: Duration = Duration::from_secs(5);
const MM_PER_INCH: f64 = 25.4;

/// Any script: while the print dialog is up the page answers none (it stays in print), the first answer after it
/// means the dialog closed. (A page event like `afterprint` never runs: page scripts are off.)
const ANSWER: &str = "true";

fn core(webview: &PlatformWebview) -> windows::core::Result<ICoreWebView2> {
    unsafe { webview.controller().CoreWebView2() }
}

/// The JSON value of `script` run in the page, waited for at most `timeout`.
fn evaluate(sheet: &Sheet, script: &'static str, timeout: Duration) -> Result<String, PrintError> {
    let rx = on_webview(sheet, move |webview, tx| unsafe {
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
            let _ = tx.send(result.map(|()| json).map_err(|e| e.to_string()));
            Ok(())
        }));
        core(webview)?.ExecuteScript(&HSTRING::from(script), &handler)
    })?;
    wait(&rx, timeout)
}

/// The loaded page's fonts and pictures are ready.
pub(super) fn settle(sheet: &Sheet) -> Result<(), PrintError> {
    until(SETTLE_TIMEOUT, || {
        Ok(evaluate(sheet, SETTLED, SETTLE_TIMEOUT)? == "true")
    })
}

/// The page as a PDF of `setup`'s paper. Margins are the page's (`@page`, where the page-number boxes live): the
/// web view's own are zero so the two never add up.
pub(super) fn pdf(sheet: &Sheet, setup: PageSetup) -> Result<Vec<u8>, PrintError> {
    let page = setup.page();
    let rx = on_webview(sheet, move |webview, tx| unsafe {
        let environment: ICoreWebView2Environment6 = webview.environment().cast()?;
        let settings = environment.CreatePrintSettings()?;
        // the page box already has the orientation applied
        settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)?;
        settings.SetPageWidth(page.width_mm / MM_PER_INCH)?;
        settings.SetPageHeight(page.height_mm / MM_PER_INCH)?;
        settings.SetMarginTop(0.0)?;
        settings.SetMarginBottom(0.0)?;
        settings.SetMarginLeft(0.0)?;
        settings.SetMarginRight(0.0)?;
        settings.SetShouldPrintBackgrounds(true)?;
        settings.SetShouldPrintHeaderAndFooter(false)?;
        let handler = PrintToPdfStreamCompletedHandler::create(Box::new(move |result, stream| {
            let bytes = result
                .and_then(|()| read_stream(stream.as_ref()))
                .map_err(|e| e.to_string());
            let _ = tx.send(bytes);
            Ok(())
        }));
        let core: ICoreWebView2_16 = core(webview)?.cast()?;
        core.PrintToPdfStream(&settings, &handler)
    })?;
    wait(&rx, RENDER_TIMEOUT)
}

/// Every byte of the stream the PDF came in.
fn read_stream(stream: Option<&IStream>) -> windows::core::Result<Vec<u8>> {
    let stream =
        stream.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; 1 << 16];
    loop {
        let mut read = 0_u32;
        let len = u32::try_from(chunk.len()).unwrap_or(u32::MAX);
        unsafe { stream.Read(chunk.as_mut_ptr().cast(), len, Some(&raw mut read)) }.ok()?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
}

/// The system print dialog for the page; returns once the dialog is done with it (printed or cancelled). The
/// paper | margins of `setup` reach it through the page's `@page` rule (the system dialog takes no settings).
///
/// The web view shows no print dialog while it counts as hidden: it is made visible to itself, its window stays
/// hidden. The system dialog places itself (`_asking` unused).
pub(super) fn print(
    sheet: &Sheet,
    _setup: PageSetup,
    _asking: Option<&tauri::Window>,
) -> Result<(), PrintError> {
    let rx = on_webview(sheet, move |webview, tx| unsafe {
        webview.controller().SetIsVisible(true)?;
        let core: ICoreWebView2_16 = core(webview)?.cast()?;
        core.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_SYSTEM)?;
        let _ = tx.send(Ok(()));
        windows::core::Result::Ok(())
    })?;
    wait(&rx, SETTLE_TIMEOUT)?;
    evaluate(sheet, ANSWER, PRINT_TIMEOUT)?;
    std::thread::sleep(SPOOL_GRACE);
    Ok(())
}
