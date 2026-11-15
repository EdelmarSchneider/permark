//! `WebKitGTK`: PDF through a print operation sent, without a dialog, to GTK's "Print to File" printer (a file in a
//! private temp folder, read back and removed); printing through the GTK print dialog. No page numbers: `WebKit`
//! draws no `@page` margin boxes.

use std::cell::RefCell;

use gtk::{PageOrientation, PaperSize, PrintSettings, Unit};
use javascriptcore::ValueExt;
use permark_core::plugins::{PageSetup, PrintCapabilities, PrintError};
use webkit2gtk::{
    PrintOperation, PrintOperationExt, PrintOperationResponse, SettingsExt, WebViewExt,
};

use super::{
    failed, on_webview, until, wait, Answer, Sheet, PRINT_TIMEOUT, RENDER_TIMEOUT, SETTLED,
    SETTLE_TIMEOUT,
};

pub const CAPABILITIES: PrintCapabilities = PrintCapabilities {
    pdf: true,
    print: true,
    page_numbers: false,
    native_margins: true,
};

/// Whether `script`, run in the page, comes out true.
fn evaluate(sheet: &Sheet, script: &'static str) -> Result<bool, PrintError> {
    let rx = on_webview(sheet, move |webview, tx| {
        webview.inner().evaluate_javascript(
            script,
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            move |result| {
                let _ = tx.send(result.map(|v| v.to_boolean()).map_err(|e| e.to_string()));
            },
        );
        Ok::<(), String>(())
    })?;
    wait(&rx, SETTLE_TIMEOUT)
}

/// The loaded page's fonts and pictures are ready.
///
/// `WebKitGTK` runs no script at all, the host's included, while scripts are off: they are turned on for the host
/// only, with every script of the page itself (elements, event attributes, `javascript:` links) still off, and turned
/// off again once the page has settled (printing needs none).
pub(super) fn settle(sheet: &Sheet) -> Result<(), PrintError> {
    scripts(sheet, true)?;
    until(SETTLE_TIMEOUT, || evaluate(sheet, SETTLED))?;
    scripts(sheet, false)
}

/// Turns the host's scripts on | off; the page's own scripts stay off either way.
fn scripts(sheet: &Sheet, on: bool) -> Result<(), PrintError> {
    let rx = on_webview(sheet, move |webview, tx| {
        let settings = WebViewExt::settings(&webview.inner())
            .ok_or_else(|| "the web view has no settings".to_owned())?;
        settings.set_enable_javascript_markup(false);
        settings.set_enable_javascript(on);
        let _ = tx.send(Ok(()));
        Ok::<(), String>(())
    })?;
    wait(&rx, SETTLE_TIMEOUT)
}

/// The paper of `setup` (its page box has the orientation applied already) with its margins: `WebKitGTK` lays the
/// page out inside these and ignores the size and margins of the page's own `@page` rule.
fn page_setup(setup: PageSetup) -> gtk::PageSetup {
    let page = setup.page();
    let paper = PaperSize::new_custom(
        "permark",
        "PerMark",
        page.width_mm,
        page.height_mm,
        Unit::Mm,
    );
    let gtk_setup = gtk::PageSetup::new();
    gtk_setup.set_paper_size(&paper);
    gtk_setup.set_orientation(PageOrientation::Portrait);
    gtk_setup.set_top_margin(page.margin_mm, Unit::Mm);
    gtk_setup.set_bottom_margin(page.margin_mm, Unit::Mm);
    gtk_setup.set_left_margin(page.margin_mm, Unit::Mm);
    gtk_setup.set_right_margin(page.margin_mm, Unit::Mm);
    gtk_setup
}

/// Sends how `operation` ended (a failure comes before its end) and keeps it alive until then.
fn report_end(operation: &PrintOperation, tx: Sender<()>) {
    let failure = tx.clone();
    operation.connect_failed(move |_, error| {
        let _ = failure.send(Err(error.to_string()));
    });
    // the operation holds this handler and the handler holds the operation: the ring opens at the end
    let held = RefCell::new(Some(operation.clone()));
    operation.connect_finished(move |_| {
        let _ = tx.send(Ok(()));
        held.take();
    });
}

type Sender<T> = std::sync::mpsc::Sender<Answer<T>>;

/// The name of GTK's file printer, in the language GTK speaks here (its print backend names it so).
fn file_printer() -> String {
    gtk::glib::dgettext(Some("gtk30"), "Print to File").to_string()
}

/// The page as a PDF of `setup`'s paper.
pub(super) fn pdf(sheet: &Sheet, setup: PageSetup) -> Result<Vec<u8>, PrintError> {
    // a folder only this user can enter (another user cannot plant a link where the printer writes)
    let folder = tempfile::Builder::new()
        .prefix("permark-export-")
        .tempdir()
        .map_err(failed)?;
    let file = folder.path().join("page.pdf");
    let uri = gtk::glib::filename_to_uri(&file, None)
        .map_err(failed)?
        .to_string();
    let rx = on_webview(sheet, move |webview, tx| {
        let operation = PrintOperation::new(&webview.inner());
        operation.set_page_setup(&page_setup(setup));
        let settings = PrintSettings::new();
        settings.set_printer(&file_printer());
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
        settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&uri));
        operation.set_print_settings(&settings);
        report_end(&operation, tx);
        operation.print();
        Ok::<(), String>(())
    })?;
    wait(&rx, RENDER_TIMEOUT)?;
    std::fs::read(&file).map_err(failed)
}

/// The GTK print dialog for the page, its paper set to `setup`; returns once the job is handed over (printed) or the
/// dialog was cancelled.
pub(super) fn print(sheet: &Sheet, setup: PageSetup) -> Result<(), PrintError> {
    let rx = on_webview(sheet, move |webview, tx| {
        let operation = PrintOperation::new(&webview.inner());
        operation.set_page_setup(&page_setup(setup));
        // the dialog runs its own loop; the job it starts ends later, on the main loop
        if operation.run_dialog(None::<&gtk::Window>) == PrintOperationResponse::Print {
            report_end(&operation, tx);
        } else {
            let _ = tx.send(Ok(()));
        }
        Ok::<(), String>(())
    })?;
    wait(&rx, PRINT_TIMEOUT)
}
