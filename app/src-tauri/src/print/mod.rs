//! Export of a finished HTML page (plugin `export`): PDF | printout through the system web view; the Save dialog, the
//! checks on the chosen path and the atomic write for PDF | HTML; opening the file just written.
//!
//! A page renders in a hidden window of its own (label `export-<n>`): it matches no capability (no IPC), runs no page
//! script, and loads from the `pm-export` scheme, which serves only the one page waiting for that window, from memory
//! (no file, no network, no size limit of a navigate-to-string).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use unsupported as native;
#[cfg(windows)]
use windows as native;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use permark_core::plugins::{
    output_path, write_output, HtmlPrinter, HtmlPrinterApi, Origin, OutputKind, PageSetup,
    PrintCapabilities, PrintError, Saved, ServiceLease,
};
use permark_core::workspace::CoreEnv;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder, Url, WebviewUrl, Wry};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

pub const SCHEME: &str = "pm-export";
/// Labels of the page windows (never a work window: `main` | `w-*`).
const LABEL_PREFIX: &str = "export-";
/// The page as served: nothing but itself and what it carries inline (no script, nothing fetched).
const PAGE_POLICY: &str = "default-src 'none'; img-src data:; media-src data:; font-src data:; \
                           style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
/// A page that never finishes loading is given up (a long document with many pictures takes a few seconds).
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// A page window of the export (not one of the person's windows).
pub fn is_sheet(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// The URL a page window loads (the scheme as each web view names it).
fn page_url() -> Option<Url> {
    let text = if cfg!(any(windows, target_os = "android")) {
        format!("http://{SCHEME}.localhost/")
    } else {
        format!("{SCHEME}://localhost/")
    };
    Url::parse(&text).ok()
}

/// A page window may go to its page only, without a query (a jump to an anchor of the page stays allowed). (Compared
/// part by part: the origin of a custom scheme URL is opaque and equals no other.)
pub fn is_page_url(url: &Url) -> bool {
    page_url().is_some_and(|page| {
        page.scheme() == url.scheme()
            && page.host_str() == url.host_str()
            && page.port_or_known_default() == url.port_or_known_default()
            && url.path() == "/"
            && url.query().is_none()
    })
}

/// The pages waiting to be served, one per page window (app state, read by the scheme handler).
#[derive(Default)]
pub struct Pages {
    pages: Mutex<HashMap<String, Arc<String>>>,
    next: AtomicU64,
}

impl Pages {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<String>>> {
        self.pages.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn add(&self, html: String) -> String {
        let label = format!(
            "{LABEL_PREFIX}{}",
            self.next.fetch_add(1, Ordering::Relaxed) + 1
        );
        self.lock().insert(label.clone(), Arc::new(html));
        label
    }

    fn page(&self, label: &str) -> Option<Arc<String>> {
        self.lock().get(label).cloned()
    }

    fn remove(&self, label: &str) {
        self.lock().remove(label);
    }
}

/// Answers a page window's request for its page; anything else (another window, another path) gets nothing.
#[allow(clippy::needless_pass_by_value)] // the signature Tauri's scheme handler requires
pub fn handle(
    context: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let pages = context.app_handle().try_state::<Pages>();
    let answer = respond(
        pages.as_deref(),
        context.webview_label(),
        request.uri().path(),
    );
    if let Ok(response) = answer {
        responder.respond(response);
    }
}

/// The answer to `label` asking for `path`: its own page at `/` when it is a page window, else 404.
fn respond(
    pages: Option<&Pages>,
    label: &str,
    path: &str,
) -> tauri::http::Result<Response<Vec<u8>>> {
    let page = (path == "/" && is_sheet(label))
        .then_some(pages)
        .flatten()
        .and_then(|pages| pages.page(label));
    match page {
        Some(html) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CONTENT_SECURITY_POLICY, PAGE_POLICY)
            .header(header::CACHE_CONTROL, "no-store")
            .body(html.as_bytes().to_vec()),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new()),
    }
}

fn failed(error: impl std::fmt::Display) -> PrintError {
    PrintError::Render(error.to_string())
}

/// Fonts and pictures of a loaded page given up on after this (data URIs: they decode, nothing is fetched).
const SETTLE_TIMEOUT: Duration = Duration::from_secs(60);
/// A long document (hundreds of pages, many pictures) still renders within this.
const RENDER_TIMEOUT: Duration = Duration::from_secs(600);
/// How long the system print dialog may stay open.
const PRINT_TIMEOUT: Duration = Duration::from_secs(3600);
const POLL: Duration = Duration::from_millis(50);

/// Every font face loaded and every picture decoded (the page runs no script of its own; this runs as the host's).
const SETTLED: &str = "document.readyState === 'complete' && document.fonts.status === 'loaded' \
                       && Array.from(document.images).every((i) => i.complete)";

/// What the web view sent back from its own thread: a value | why it failed.
type Answer<T> = Result<T, String>;

/// Runs `start` on the UI thread with the page's web view; its answer comes through the channel it is given.
fn on_webview<T: Send + 'static, E: std::fmt::Display>(
    sheet: &Sheet,
    start: impl FnOnce(&tauri::webview::PlatformWebview, Sender<Answer<T>>) -> Result<(), E>
        + Send
        + 'static,
) -> Result<Receiver<Answer<T>>, PrintError> {
    let (tx, rx) = mpsc::channel();
    sheet
        .window()?
        .with_webview(move |webview| {
            if let Err(error) = start(&webview, tx.clone()) {
                let _ = tx.send(Err(error.to_string()));
            }
        })
        .map_err(failed)?;
    Ok(rx)
}

fn wait<T>(rx: &Receiver<Answer<T>>, timeout: Duration) -> Result<T, PrintError> {
    rx.recv_timeout(timeout)
        .map_err(|_| failed("the web view did not answer"))?
        .map_err(PrintError::Render)
}

/// Polls `ready` until it says yes, at most `timeout`.
fn until(
    timeout: Duration,
    mut ready: impl FnMut() -> Result<bool, PrintError>,
) -> Result<(), PrintError> {
    let deadline = Instant::now() + timeout;
    while !ready()? {
        if Instant::now() >= deadline {
            return Err(failed("the page did not settle"));
        }
        std::thread::sleep(POLL);
    }
    Ok(())
}

/// A page loaded in its own hidden window; dropping it closes the window and forgets the page.
pub(crate) struct Sheet {
    handle: AppHandle,
    label: String,
    window: Option<tauri::WebviewWindow>,
}

impl Sheet {
    /// Opens `html` in a new hidden page window and waits until it has loaded with its fonts and pictures. Never on
    /// the main thread (the window is made with a round trip to it). `data_dir`: the web view data folder of the
    /// app's views (one browser process for both; the web view refuses one folder with two option sets).
    ///
    /// # Errors
    /// No page store | the window could not be made | the page did not load in time.
    pub(crate) fn open(
        handle: &AppHandle,
        html: String,
        data_dir: Option<PathBuf>,
    ) -> Result<Self, PrintError> {
        let pages = handle.try_state::<Pages>().ok_or(PrintError::Unsupported)?;
        let url = page_url().ok_or(PrintError::Unsupported)?;
        let mut sheet = Self {
            handle: handle.clone(),
            label: pages.add(html),
            window: None,
        };
        let (tx, rx) = mpsc::channel();
        let builder =
            WebviewWindowBuilder::new(handle, &sheet.label, WebviewUrl::CustomProtocol(url))
                .title("PerMark")
                .inner_size(900.0, 1200.0)
                .visible(false)
                .focused(false)
                .skip_taskbar(true)
                .disable_javascript()
                .on_navigation(is_page_url)
                .on_new_window(|_url, _features| NewWindowResponse::Deny)
                .on_page_load(move |_webview, payload| {
                    if payload.event() == PageLoadEvent::Finished {
                        let _ = tx.send(());
                    }
                });
        #[cfg(not(target_os = "macos"))]
        let builder = match data_dir {
            Some(dir) => builder.data_directory(dir),
            None => builder,
        };
        // WKWebView takes no data folder; `let _` would not consume the value (clippy `needless_pass_by_value`)
        #[cfg(target_os = "macos")]
        drop(data_dir);
        sheet.window = Some(builder.build().map_err(failed)?);
        rx.recv_timeout(LOAD_TIMEOUT)
            .map_err(|_| failed("the page did not load"))?;
        native::settle(&sheet)?;
        Ok(sheet)
    }

    pub(crate) fn window(&self) -> Result<&tauri::WebviewWindow, PrintError> {
        self.window.as_ref().ok_or(PrintError::Unsupported)
    }
}

impl Drop for Sheet {
    fn drop(&mut self) {
        if let Some(window) = self.window.take() {
            let _ = window.destroy();
        }
        if let Some(pages) = self.handle.try_state::<Pages>() {
            pages.remove(&self.label);
        }
    }
}

/// Window -> the file last written for it: the only file Open | Show in folder ever touch. A closed window's entry goes
/// with it.
#[derive(Default)]
struct Written(Mutex<HashMap<String, PathBuf>>);

impl Written {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, PathBuf>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn keep(&self, window: &str, path: PathBuf) {
        self.lock().insert(window.to_owned(), path);
    }

    /// The file last written for `window`, still on disk.
    fn last(&self, window: &str) -> Result<PathBuf, PrintError> {
        let path = self
            .lock()
            .get(window)
            .cloned()
            .ok_or(PrintError::NothingWritten)?;
        if path.is_file() {
            Ok(path)
        } else {
            Err(PrintError::Gone(path))
        }
    }

    fn forget(&self, window: &str) {
        self.lock().remove(window);
    }
}

/// The shell's printer: renders through the system web view, writes what the person chose.
struct ShellPrinter {
    handle: AppHandle,
    /// The web view data folder of the app's views.
    data_dir: Option<PathBuf>,
    written: Written,
}

impl ShellPrinter {
    /// The window holding the view `label` (a dialog for that view sits on it).
    fn window_of(&self, label: &str) -> Option<tauri::Window> {
        self.handle.get_webview(label).map(|view| view.window())
    }

    /// The Save dialog on the asking window, in the source's folder with the source's name; the chosen path checked.
    /// `None`: the person cancelled.
    fn ask(&self, origin: &Origin, kind: OutputKind) -> Result<Option<PathBuf>, PrintError> {
        let mut dialog = self
            .handle
            .dialog()
            .file()
            .set_file_name(origin.suggested_name(kind))
            // the filter is also the default extension: Windows and macOS add it themselves and ask before replacing
            // that file; where the dialog cannot (GTK), `output_path` refuses an added name that exists
            .add_filter(kind.ext().to_uppercase(), &[kind.ext()]);
        if let Some(folder) = origin.folder() {
            dialog = dialog.set_directory(folder);
        }
        if let Some(window) = self.window_of(&origin.window) {
            dialog = dialog.set_parent(&window);
        }
        let Some(chosen) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let chosen = chosen
            .into_path()
            .map_err(|e| PrintError::BadTarget(e.to_string()))?;
        output_path(&chosen, kind, &origin.source).map(Some)
    }

    fn keep(&self, origin: &Origin, path: PathBuf) -> Saved {
        self.written.keep(&origin.window, path.clone());
        Saved::Saved { path }
    }
}

impl HtmlPrinter for ShellPrinter {
    fn capabilities(&self) -> PrintCapabilities {
        native::capabilities()
    }

    fn save_pdf(
        &self,
        html: String,
        setup: &PageSetup,
        origin: &Origin,
        cancel: &AtomicBool,
    ) -> Result<Saved, PrintError> {
        if !native::CAPABILITIES.pdf {
            return Err(PrintError::Unsupported);
        }
        let Some(path) = self.ask(origin, OutputKind::Pdf)? else {
            return Ok(Saved::Cancelled);
        };
        let bytes = {
            let sheet = Sheet::open(&self.handle, html, self.data_dir.clone())?;
            native::pdf(&sheet, *setup)?
        };
        if cancel.load(Ordering::SeqCst) {
            return Ok(Saved::Cancelled);
        }
        write_output(&path, &bytes)?;
        Ok(self.keep(origin, path))
    }

    fn save_html(
        &self,
        html: &str,
        origin: &Origin,
        cancel: &AtomicBool,
    ) -> Result<Saved, PrintError> {
        let Some(path) = self.ask(origin, OutputKind::Html)? else {
            return Ok(Saved::Cancelled);
        };
        // cancelled while the dialog was open: the person's Cancel wins over the path chosen in it
        if cancel.load(Ordering::SeqCst) {
            return Ok(Saved::Cancelled);
        }
        write_output(&path, html.as_bytes())?;
        Ok(self.keep(origin, path))
    }

    fn print(
        &self,
        html: String,
        setup: &PageSetup,
        window: &str,
        cancel: &AtomicBool,
    ) -> Result<(), PrintError> {
        if !native::CAPABILITIES.print {
            return Err(PrintError::Unsupported);
        }
        let sheet = Sheet::open(&self.handle, html, self.data_dir.clone())?;
        // the page loads before any dialog shows: a Cancel meanwhile means no dialog at all
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        native::print(&sheet, *setup, self.window_of(window).as_ref())
    }

    fn forget(&self, window: &str) {
        self.written.forget(window);
    }

    fn open(&self, window: &str) -> Result<(), PrintError> {
        let path = self.written.last(window)?;
        // a path this printer wrote itself, ending in `.pdf` | `.html`: never a program
        self.handle
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| PrintError::Opener {
                path: path.clone(),
                detail: e.to_string(),
            })
    }

    fn reveal(&self, window: &str) -> Result<(), PrintError> {
        let path = self.written.last(window)?;
        self.handle
            .opener()
            .reveal_item_in_dir(&path)
            .map_err(|e| PrintError::Opener {
                path: path.clone(),
                detail: e.to_string(),
            })
    }
}

/// Offers the printer for the app's whole run.
pub fn provide(env: &CoreEnv, handle: &AppHandle) -> ServiceLease {
    let printer = ShellPrinter {
        handle: handle.clone(),
        data_dir: crate::views::data_directory(env),
        written: Written::default(),
    };
    env.services()
        .provide(Arc::new(HtmlPrinterApi(Arc::new(printer))))
}

#[cfg(test)]
mod tests;
