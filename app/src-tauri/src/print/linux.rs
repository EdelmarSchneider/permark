//! `WebKitGTK`: PDF through a print operation sent, without a dialog, to GTK's "Print to File" printer (a file in a
//! private temp folder, read back and removed); printing through the GTK print dialog. No page numbers: `WebKit`
//! draws no `@page` margin boxes.

use std::cell::RefCell;

use gtk::{PageOrientation, PaperSize, PrintSettings, Unit};
use javascriptcore::ValueExt;
use permark_core::plugins::{Orientation, PageSetup, PageSize, PrintCapabilities, PrintError};
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
    snap_needs_cups: false,
};

/// Where a snap's `cups` connection puts the CUPS snap's socket (inside the snap's own view of the files).
const SNAP_CUPS_SOCKET: &str = "/var/cups/cups.sock";

/// Read at each call: the person may install the CUPS snap while the app runs.
pub fn capabilities() -> PrintCapabilities {
    PrintCapabilities {
        snap_needs_cups: snap_lacks_cups(
            std::env::var_os("SNAP").is_some(),
            std::path::Path::new(SNAP_CUPS_SOCKET).exists(),
        ),
        ..CAPABILITIES
    }
}

/// A snap reaches printers only through the CUPS snap's socket; outside a snap the system's CUPS is used directly.
const fn snap_lacks_cups(in_snap: bool, socket_present: bool) -> bool {
    in_snap && !socket_present
}

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

/// Prints 1 CSS px as 0.75 pt (1/96 in), as the other web views do. `WebKit` lays a printed page out 1.25 times as
/// wide as its printable width taken in points (`WebCore`'s smallest print shrink factor): unzoomed, 1 CSS px prints
/// as 0.8 pt, text 7% larger with shorter lines and more pages. Zoom 0.75 / 0.8 = 0.9375 gives the same text size,
/// line length and page breaks; a page wider than that still shrinks to fit as before. (The web view's own zoom level
/// does not reach printing.)
const PRINT_SCALE: &str = "document.documentElement.style.zoom = '0.9375'; true";

/// The loaded page's fonts and pictures are ready, the page scaled for print ([`PRINT_SCALE`]).
///
/// `WebKitGTK` runs no script at all, the host's included, while scripts are off: they are turned on for the host
/// only, with every script of the page itself (elements, event attributes, `javascript:` links) still off, and turned
/// off again once the page has settled (printing needs none).
pub(super) fn settle(sheet: &Sheet) -> Result<(), PrintError> {
    scripts(sheet, true)?;
    until(SETTLE_TIMEOUT, || evaluate(sheet, SETTLED))?;
    if !evaluate(sheet, PRINT_SCALE)? {
        return Err(failed("the page could not be scaled for print"));
    }
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

/// GTK's standard name of `setup`'s paper (the PWG name a printer knows: no question about paper or tray) and its
/// orientation.
fn paper(setup: PageSetup) -> (&'static str, PageOrientation) {
    let name = match setup.size {
        PageSize::A3 => "iso_a3",
        PageSize::A4 => "iso_a4",
        PageSize::A5 => "iso_a5",
        PageSize::Letter => "na_letter",
        PageSize::Legal => "na_legal",
        // 11 x 17 in: shown as "Tabloid"
        PageSize::Tabloid => "na_ledger",
    };
    let orientation = match setup.orientation {
        Orientation::Portrait => PageOrientation::Portrait,
        Orientation::Landscape => PageOrientation::Landscape,
    };
    (name, orientation)
}

/// The paper of `setup` with its margins: `WebKitGTK` draws the page inside these and ignores the size of the page's
/// own `@page` rule.
fn page_setup(setup: PageSetup) -> gtk::PageSetup {
    let (name, orientation) = paper(setup);
    let margin = setup.page().margin_mm;
    let gtk_setup = gtk::PageSetup::new();
    gtk_setup.set_paper_size(&PaperSize::new(Some(name)));
    gtk_setup.set_orientation(orientation);
    gtk_setup.set_top_margin(margin, Unit::Mm);
    gtk_setup.set_bottom_margin(margin, Unit::Mm);
    gtk_setup.set_left_margin(margin, Unit::Mm);
    gtk_setup.set_right_margin(margin, Unit::Mm);
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

/// The GTK print dialog for the page, its paper set to `setup`, modal on `asking` (the window that asked: it stays in
/// front of it); returns once the job is handed over (printed) or the dialog was cancelled.
///
/// The dialog runs its own loop on the UI thread until it closes (`WebKitGTK` offers no other way to start a print
/// with the person's choices; GTK's non-blocking dialog has no binding here): app tasks for the UI thread wait until
/// then.
pub(super) fn print(
    sheet: &Sheet,
    setup: PageSetup,
    asking: Option<&tauri::Window>,
) -> Result<(), PrintError> {
    let asking = asking.cloned();
    let rx = on_webview(sheet, move |webview, tx| {
        let operation = PrintOperation::new(&webview.inner());
        operation.set_page_setup(&page_setup(setup));
        let parent = asking.and_then(|window| window.gtk_window().ok());
        // the job the dialog starts ends later, on the main loop
        if operation.run_dialog(parent.as_ref()) == PrintOperationResponse::Print {
            report_end(&operation, tx);
        } else {
            let _ = tx.send(Ok(()));
        }
        Ok::<(), String>(())
    })?;
    wait(&rx, PRINT_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use permark_core::plugins::Margins;

    use super::*;

    #[test]
    fn only_a_snap_without_the_cups_socket_needs_the_cups_snap() {
        assert!(snap_lacks_cups(true, false));
        assert!(!snap_lacks_cups(true, true));
        assert!(!snap_lacks_cups(false, false));
        assert!(!snap_lacks_cups(false, true));
    }

    const SIZES: [PageSize; 6] = [
        PageSize::A3,
        PageSize::A4,
        PageSize::A5,
        PageSize::Letter,
        PageSize::Legal,
        PageSize::Tabloid,
    ];

    fn setup(size: PageSize, orientation: Orientation) -> PageSetup {
        PageSetup {
            size,
            orientation,
            margins: Margins::Wide,
            page_numbers: false,
        }
    }

    #[test]
    fn each_paper_goes_to_gtk_by_its_standard_name_and_orientation() {
        let names = SIZES.map(|size| paper(setup(size, Orientation::Portrait)).0);
        assert_eq!(
            names,
            [
                "iso_a3",
                "iso_a4",
                "iso_a5",
                "na_letter",
                "na_legal",
                "na_ledger"
            ]
        );
        for size in SIZES {
            assert_eq!(
                paper(setup(size, Orientation::Portrait)).1,
                PageOrientation::Portrait
            );
            assert_eq!(
                paper(setup(size, Orientation::Landscape)).1,
                PageOrientation::Landscape
            );
        }
    }

    /// GTK falls back to A4 for a name it does not know: each name must give the page box's own size.
    #[test]
    #[ignore = "needs a display (GTK)"]
    fn each_gtk_paper_has_the_page_box_size_and_margins() {
        gtk::init().unwrap();
        let near = |a: f64, b: f64| (a - b).abs() < 0.05;
        for size in SIZES {
            for orientation in [Orientation::Portrait, Orientation::Landscape] {
                let setup = setup(size, orientation);
                let page = setup.page();
                let gtk_setup = page_setup(setup);
                let gtk_paper = gtk_setup.paper_size();
                assert!(!gtk_paper.is_custom(), "{size:?}");
                assert_eq!(
                    gtk_paper.name().as_deref(),
                    Some(paper(setup).0),
                    "{size:?}"
                );
                assert!(
                    near(gtk_setup.paper_width(Unit::Mm), page.width_mm)
                        && near(gtk_setup.paper_height(Unit::Mm), page.height_mm),
                    "{size:?} {orientation:?}"
                );
                for margin in [
                    gtk_setup.top_margin(Unit::Mm),
                    gtk_setup.bottom_margin(Unit::Mm),
                    gtk_setup.left_margin(Unit::Mm),
                    gtk_setup.right_margin(Unit::Mm),
                ] {
                    assert!(near(margin, page.margin_mm), "{size:?} {orientation:?}");
                }
            }
        }
    }
}
