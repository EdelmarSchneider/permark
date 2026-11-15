//! Systems without a print backend here (neither `WebView2`, `WebKitGTK` on Linux nor `WKWebView` on macOS): PDF and
//! printing are not available (each call answers `Unsupported`; the export plugin shows that).

use permark_core::plugins::{PageSetup, PrintCapabilities, PrintError};

use super::Sheet;

pub const CAPABILITIES: PrintCapabilities = PrintCapabilities {
    pdf: false,
    print: false,
    page_numbers: false,
};

pub(super) fn settle(_sheet: &Sheet) -> Result<(), PrintError> {
    Err(PrintError::Unsupported)
}

pub(super) fn pdf(_sheet: &Sheet, _setup: PageSetup) -> Result<Vec<u8>, PrintError> {
    Err(PrintError::Unsupported)
}

pub(super) fn print(_sheet: &Sheet, _setup: PageSetup) -> Result<(), PrintError> {
    Err(PrintError::Unsupported)
}
