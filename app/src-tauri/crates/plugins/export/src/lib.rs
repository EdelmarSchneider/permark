//! Plugin `export`: the open Markdown document as a PDF | a standalone HTML page, or printed. The window builds the
//! finished page; the shell's [`HtmlPrinterApi`] renders it in the system web view and owns the Save dialog, the
//! checks on the chosen path and the atomic write. The page setup (paper, orientation, margins, page numbers) is
//! remembered app-wide in the plugin's own state file.
//!
//! RPC: `setup.get` | `setup.set`, `capabilities`, `saveHtml`, `savePdf`, `print`, `cancel` (the window's run in
//! progress: nothing written | no print dialog; a file already written stays `saved`), `open` | `reveal` (the file
//! last written for the window, nothing else).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, AppServices, HtmlPrinterApi, Manifest, NoArgs, Origin, PageBox, PageSetup, Plugin,
    PluginCx, PluginError, PluginId, PluginKind, PluginScope, PrintCapabilities, RpcRegistration,
    Running, Saved, WindowFilesApi,
};
use permark_core::state::{StateData, StateFile};
use permark_core::{ErrorKind, IpcError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const ID: PluginId = PluginId::of("export");

/// What `setup.json` keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Remembered {
    setup: PageSetup,
}

impl StateData for Remembered {
    const VERSION: u32 = 1;
}

/// The page setup to offer, with the page box it gives (the numbers of the page's `@page` rule).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetupView {
    pub setup: PageSetup,
    pub page: PageBox,
    /// The person chose it once (`false`: the default; the window may pick the paper of the person's region).
    pub stored: bool,
}

impl SetupView {
    fn of(setup: PageSetup, stored: bool) -> Self {
        Self {
            setup,
            page: setup.page(),
            stored,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetupArgs {
    setup: PageSetup,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HtmlArgs {
    html: String,
    /// The document's path in the window's workspace.
    source: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PdfArgs {
    html: String,
    source: String,
    setup: PageSetup,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrintArgs {
    html: String,
    setup: PageSetup,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(Export {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

struct Export {
    manifest: Manifest,
}

struct Shared {
    app: AppServices,
    file: Option<StateFile<Remembered>>,
    /// Window -> the cancel flag of its run in progress (PDF | HTML | print).
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Shared {
    fn printer(&self) -> Result<Arc<HtmlPrinterApi>, IpcError> {
        self.app
            .services
            .get::<HtmlPrinterApi>()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("print"))
    }

    fn capabilities(&self) -> PrintCapabilities {
        self.printer()
            .map(|p| p.0.capabilities())
            .unwrap_or_default()
    }

    fn setup(&self) -> SetupView {
        match self.file.as_ref().map(StateFile::load) {
            Some(permark_core::state::Loaded::Found(data)) => SetupView::of(data.setup, true),
            _ => SetupView::of(PageSetup::default(), false),
        }
    }

    fn set_setup(&self, setup: PageSetup) -> Result<SetupView, IpcError> {
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("state"))?;
        file.save(&Remembered { setup })?;
        Ok(SetupView::of(setup, true))
    }

    /// The document on disk, checked inside the window's workspace (only the dialog's defaults and the same-file
    /// refusal use it).
    fn origin(&self, window: &str, source: &str) -> Result<Origin, IpcError> {
        let files = self
            .app
            .services
            .get::<WindowFilesApi>()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("files"))?;
        Ok(Origin {
            window: window.to_owned(),
            source: files.0.locate(window, source)?,
        })
    }

    /// Runs `job` with the window's cancel flag registered for its whole run (`cancel` reaches it).
    fn cancellable<T>(&self, window: &str, job: impl FnOnce(&AtomicBool) -> T) -> T {
        let cancel = Arc::new(AtomicBool::new(false));
        self.lock().insert(window.to_owned(), Arc::clone(&cancel));
        let result = job(&cancel);
        let mut running = self.lock();
        if running.get(window).is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
            running.remove(window);
        }
        result
    }

    fn save_pdf(&self, args: PdfArgs, window: &str) -> Result<Saved, IpcError> {
        let printer = self.printer()?;
        let origin = self.origin(window, &args.source)?;
        let saved = self.cancellable(window, |cancel| {
            printer.0.save_pdf(args.html, &args.setup, &origin, cancel)
        });
        Ok(saved?)
    }

    fn save_html(&self, args: &HtmlArgs, window: &str) -> Result<Saved, IpcError> {
        let printer = self.printer()?;
        let origin = self.origin(window, &args.source)?;
        let saved = self.cancellable(window, |cancel| {
            printer.0.save_html(&args.html, &origin, cancel)
        });
        Ok(saved?)
    }

    fn print(&self, args: PrintArgs, window: &str) -> Result<(), IpcError> {
        let printer = self.printer()?;
        let outcome = self.cancellable(window, |cancel| {
            printer.0.print(args.html, &args.setup, window, cancel)
        });
        Ok(outcome?)
    }

    fn cancel(&self, window: &str) {
        if let Some(cancel) = self.lock().get(window) {
            cancel.store(true, Ordering::SeqCst);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Started {
    _rpc: Vec<RpcRegistration>,
}

impl Running for Started {}

impl Plugin for Export {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let app = cx.app().cloned().ok_or_else(|| PluginError::Start {
            id: ID,
            reason: "needs the app services".to_owned(),
        })?;
        let shared = Arc::new(Shared {
            app,
            file: cx.state_file::<Remembered>("setup.json"),
            running: Mutex::new(HashMap::new()),
        });
        let mut rpc = Vec::new();

        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc("setup.get", typed(move |_: NoArgs, _| Ok(s.setup()))));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "setup.set",
            typed(move |args: SetupArgs, _| s.set_setup(args.setup)),
        ));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "capabilities",
            typed(move |_: NoArgs, _| Ok(s.capabilities())),
        ));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "saveHtml",
            typed(move |args: HtmlArgs, call| s.save_html(&args, &call.window)),
        ));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "savePdf",
            typed(move |args: PdfArgs, call| s.save_pdf(args, &call.window)),
        ));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "print",
            typed(move |args: PrintArgs, call| s.print(args, &call.window)),
        ));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "cancel",
            typed(move |_: NoArgs, call| {
                s.cancel(&call.window);
                Ok(())
            }),
        ));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "open",
            typed(move |_: NoArgs, call| Ok(s.printer()?.0.open(&call.window)?)),
        ));
        let s = Arc::clone(&shared);
        rpc.extend(cx.rpc(
            "reveal",
            typed(move |_: NoArgs, call| Ok(s.printer()?.0.reveal(&call.window)?)),
        ));

        Ok(Box::new(Started { _rpc: rpc }))
    }
}

#[cfg(test)]
mod tests;
