use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::RwLock;

use permark_core::conditions::{Context, Distribution};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{
    CxWorkspace, HtmlPrinter, Margins, Orientation, PageSize, PrintError, RpcCall, RpcTable,
    ServiceLease, Services, WindowFiles,
};
use permark_core::settings::AppSettings;
use permark_core::state::{AppDirs, StateStore};
use serde_json::{json, Value};

use super::*;

struct Quiet;
impl EventSink for Quiet {
    fn emit(&self, _: CoreEvent) {}
}

/// A workspace with one document, `docs/a.md`, under `root`; records the files announced as written.
struct Files {
    root: PathBuf,
    announced: Mutex<Vec<(String, PathBuf)>>,
}

impl WindowFiles for Files {
    fn read(&self, _window: &str, _path: &str, _cap: u64) -> Result<String, IpcError> {
        Err(IpcError::new(ErrorKind::Unsupported))
    }

    fn locate(&self, _window: &str, path: &str) -> Result<PathBuf, IpcError> {
        if path == "docs/a.md" {
            Ok(self.root.join("docs").join("a.md"))
        } else {
            Err(IpcError::new(ErrorKind::PathEscape).subject(path))
        }
    }

    fn write_beside(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, IpcError> {
        Err(IpcError::new(ErrorKind::Unsupported))
    }

    fn note(&self, _window: &str, _fact: permark_core::activity::Fact) {}

    fn announce_written(&self, window: &str, abs: &std::path::Path) {
        self.announced
            .lock()
            .unwrap()
            .push((window.to_owned(), abs.to_path_buf()));
    }
}

/// What the printer was asked.
#[derive(Debug, Clone, PartialEq)]
enum Asked {
    Pdf {
        html: String,
        setup: PageSetup,
        origin: Origin,
    },
    Html {
        html: String,
        origin: Origin,
    },
    Print {
        html: String,
        setup: PageSetup,
        window: String,
    },
    Open(String),
    Reveal(String),
    /// The run before this one saw its cancel flag set and stopped.
    Dropped,
}

/// A printer that records; a run waits for `release` when one is given (a dialog open | a render in progress).
#[derive(Default)]
struct Printer {
    asked: Mutex<Vec<Asked>>,
    release: Mutex<Option<mpsc::Receiver<()>>>,
    started: Mutex<Option<mpsc::Sender<()>>>,
}

impl HtmlPrinter for Printer {
    fn capabilities(&self) -> PrintCapabilities {
        PrintCapabilities {
            pdf: true,
            print: true,
            page_numbers: true,
            native_margins: false,
            snap_needs_cups: false,
        }
    }

    fn save_pdf(
        &self,
        html: String,
        setup: &PageSetup,
        origin: &Origin,
        cancel: &AtomicBool,
    ) -> Result<Saved, PrintError> {
        self.asked.lock().unwrap().push(Asked::Pdf {
            html,
            setup: *setup,
            origin: origin.clone(),
        });
        Ok(self.finish(cancel, origin.source.with_extension("pdf")))
    }

    fn save_html(
        &self,
        html: &str,
        origin: &Origin,
        cancel: &AtomicBool,
    ) -> Result<Saved, PrintError> {
        self.asked.lock().unwrap().push(Asked::Html {
            html: html.to_owned(),
            origin: origin.clone(),
        });
        Ok(self.finish(cancel, origin.source.with_extension("html")))
    }

    fn print(
        &self,
        html: String,
        setup: &PageSetup,
        window: &str,
        cancel: &AtomicBool,
    ) -> Result<(), PrintError> {
        self.asked.lock().unwrap().push(Asked::Print {
            html,
            setup: *setup,
            window: window.to_owned(),
        });
        self.finish(cancel, PathBuf::new());
        Ok(())
    }

    fn forget(&self, _window: &str) {}

    fn open(&self, window: &str) -> Result<(), PrintError> {
        self.asked
            .lock()
            .unwrap()
            .push(Asked::Open(window.to_owned()));
        Err(PrintError::NothingWritten)
    }

    fn reveal(&self, window: &str) -> Result<(), PrintError> {
        self.asked
            .lock()
            .unwrap()
            .push(Asked::Reveal(window.to_owned()));
        Ok(())
    }
}

impl Printer {
    /// Waits for `release` (when given), then writes to `path` unless the run was cancelled meanwhile.
    fn finish(&self, cancel: &AtomicBool, path: PathBuf) -> Saved {
        if let Some(started) = self.started.lock().unwrap().take() {
            started.send(()).unwrap();
        }
        if let Some(release) = self.release.lock().unwrap().take() {
            release.recv().unwrap();
        }
        if cancel.load(Ordering::SeqCst) {
            self.asked.lock().unwrap().push(Asked::Dropped);
            return Saved::Cancelled;
        }
        Saved::Saved { path }
    }
}

struct Harness {
    rpc: Arc<RpcTable>,
    printer: Arc<Printer>,
    files: Arc<Files>,
    root: tempfile::TempDir,
    running: Option<Box<dyn Running>>,
    _leases: Vec<ServiceLease>,
}

impl Harness {
    fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        Self::start_in(root)
    }

    /// Started again over the same profile (what an app restart sees).
    fn start_in(root: tempfile::TempDir) -> Self {
        let rpc = RpcTable::new();
        let services = Services::new();
        let printer = Arc::new(Printer::default());
        let files = Arc::new(Files {
            root: root.path().to_path_buf(),
            announced: Mutex::default(),
        });
        let leases = vec![
            services.provide(Arc::new(HtmlPrinterApi(
                Arc::clone(&printer) as Arc<dyn HtmlPrinter>
            ))),
            services.provide(Arc::new(WindowFilesApi(
                Arc::clone(&files) as Arc<dyn WindowFiles>
            ))),
        ];
        let dirs = AppDirs::under(&root.path().join("profile"));
        let cx = PluginCx::new(
            ID,
            CxWorkspace::App,
            dirs.profile.join("plugins").join("export"),
            None,
            Context::welcome(Distribution::Direct),
            Arc::new(Quiet),
        )
        .with_store(StateStore::app(&dirs))
        .with_app(AppServices::new(
            Arc::clone(&rpc),
            services,
            Arc::new(RwLock::new(AppSettings::default())),
        ));
        let running = plugin().start(&cx).unwrap();
        Self {
            rpc,
            printer,
            files,
            root,
            running: Some(running),
            _leases: leases,
        }
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, IpcError> {
        call(&self.rpc, "w-1", method, args)
    }

    fn asked(&self) -> Vec<Asked> {
        self.printer.asked.lock().unwrap().clone()
    }

    fn source(&self) -> PathBuf {
        self.root.path().join("docs").join("a.md")
    }
}

fn call(rpc: &RpcTable, window: &str, method: &str, args: Value) -> Result<Value, IpcError> {
    rpc.call(
        "export",
        method,
        &RpcCall {
            window: window.into(),
            args,
        },
    )
}

#[test]
fn the_setup_starts_as_the_default_and_is_remembered_across_starts() {
    let h = Harness::start();
    let first = h.call("setup.get", Value::Null).unwrap();
    assert_eq!(
        first,
        json!({
            "setup": { "size": "a4", "orientation": "portrait", "margins": "normal", "pageNumbers": true },
            "page": { "widthMm": 210.0, "heightMm": 297.0, "marginMm": 20.0 },
            "stored": false
        })
    );
    let chosen = json!({ "size": "letter", "orientation": "landscape", "margins": "narrow", "pageNumbers": false });
    let set = h.call("setup.set", json!({ "setup": chosen })).unwrap();
    assert_eq!(set["stored"], true);
    assert_eq!(
        set["page"],
        json!({ "widthMm": 279.4, "heightMm": 215.9, "marginMm": 10.0 })
    );
    // an app restart reads it back
    let root = {
        let Harness { root, .. } = h;
        root
    };
    let again = Harness::start_in(root);
    let read = again.call("setup.get", Value::Null).unwrap();
    assert_eq!(read["setup"], chosen);
    assert_eq!(read["stored"], true);
    // a setup the plugin does not know is refused, the stored one kept
    let bad = again
        .call("setup.set", json!({ "setup": { "size": "b5", "orientation": "portrait", "margins": "normal", "pageNumbers": true } }))
        .unwrap_err();
    assert_eq!(bad.kind, ErrorKind::InvalidData);
    assert_eq!(
        again.call("setup.get", Value::Null).unwrap()["setup"],
        chosen
    );
}

#[test]
fn a_pdf_names_the_document_on_disk_and_the_asking_window() {
    let h = Harness::start();
    let setup = PageSetup {
        size: PageSize::A5,
        orientation: Orientation::Landscape,
        margins: Margins::Wide,
        page_numbers: false,
    };
    let saved = h
        .call(
            "savePdf",
            json!({ "html": "<p>x</p>", "source": "docs/a.md", "setup": setup }),
        )
        .unwrap();
    let pdf = h.source().with_extension("pdf");
    assert_eq!(
        saved,
        json!({ "outcome": "saved", "path": pdf.display().to_string() })
    );
    assert_eq!(
        h.asked(),
        [Asked::Pdf {
            html: "<p>x</p>".into(),
            setup,
            origin: Origin {
                window: "w-1".into(),
                source: h.source(),
            },
        }]
    );
}

#[test]
fn a_source_outside_the_window_s_workspace_reaches_no_printer() {
    let h = Harness::start();
    let refused = h
        .call(
            "saveHtml",
            json!({ "html": "<p>x</p>", "source": "../secret.md" }),
        )
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::PathEscape);
    let refused = h
        .call(
            "savePdf",
            json!({ "html": "x", "source": "../secret.md", "setup": PageSetup::default() }),
        )
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::PathEscape);
    assert_eq!(h.asked(), []);
}

#[test]
fn html_print_open_and_reveal_go_to_the_printer_for_the_asking_window() {
    let h = Harness::start();
    assert_eq!(
        h.call(
            "saveHtml",
            json!({ "html": "<p>h</p>", "source": "docs/a.md" })
        )
        .unwrap(),
        json!({ "outcome": "saved", "path": h.source().with_extension("html").display().to_string() })
    );
    h.call(
        "print",
        json!({ "html": "<p>p</p>", "setup": PageSetup::default() }),
    )
    .unwrap();
    assert_eq!(
        h.call("open", Value::Null).unwrap_err().kind,
        ErrorKind::NotFound
    );
    h.call("reveal", Value::Null).unwrap();
    assert_eq!(
        h.asked(),
        [
            Asked::Html {
                html: "<p>h</p>".into(),
                origin: Origin {
                    window: "w-1".into(),
                    source: h.source()
                }
            },
            // the print dialog sits on the window that asked
            Asked::Print {
                html: "<p>p</p>".into(),
                setup: PageSetup::default(),
                window: "w-1".into()
            },
            Asked::Open("w-1".into()),
            Asked::Reveal("w-1".into()),
        ]
    );
    assert_eq!(
        h.call("capabilities", Value::Null).unwrap(),
        json!({ "pdf": true, "print": true, "pageNumbers": true, "nativeMargins": false, "snapNeedsCups": false })
    );
}

/// A run of window `w-1` (`method` with `args`) in progress while `canceller` asks to cancel: what the call answers
/// and whether the printer stopped before its output.
fn cancelled_by(method: &'static str, args: Value, canceller: &str) -> (Value, bool) {
    cancelled_in(&Harness::start(), method, args, canceller)
}

/// [`cancelled_by`] on the harness `h`.
fn cancelled_in(h: &Harness, method: &'static str, args: Value, canceller: &str) -> (Value, bool) {
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    *h.printer.release.lock().unwrap() = Some(release_rx);
    *h.printer.started.lock().unwrap() = Some(started_tx);
    let rpc = Arc::clone(&h.rpc);
    let worker = std::thread::spawn(move || call(&rpc, "w-1", method, args));
    started_rx.recv().unwrap();
    call(&h.rpc, canceller, "cancel", Value::Null).unwrap();
    release_tx.send(()).unwrap();
    let answer = worker.join().unwrap().unwrap();
    (answer, h.asked().contains(&Asked::Dropped))
}

#[test]
fn cancel_stops_the_run_of_its_own_window_only() {
    let pdf = || json!({ "html": "x", "source": "docs/a.md", "setup": PageSetup::default() });
    let html = || json!({ "html": "x", "source": "docs/a.md" });
    let print = || json!({ "html": "x", "setup": PageSetup::default() });
    for (method, args) in [("savePdf", pdf()), ("saveHtml", html())] {
        assert_eq!(
            cancelled_by(method, args.clone(), "w-1"),
            (json!({ "outcome": "cancelled" }), true),
            "{method}"
        );
        let (answer, dropped) = cancelled_by(method, args, "w-2");
        assert_eq!(
            (answer["outcome"].as_str(), dropped),
            (Some("saved"), false),
            "{method}"
        );
    }
    assert_eq!(cancelled_by("print", print(), "w-1"), (Value::Null, true));
    assert_eq!(cancelled_by("print", print(), "w-2"), (Value::Null, false));
}

#[test]
fn a_saved_file_is_announced_to_the_asking_window_s_workspace_and_a_cancelled_one_is_not() {
    let h = Harness::start();
    h.call(
        "savePdf",
        json!({ "html": "x", "source": "docs/a.md", "setup": PageSetup::default() }),
    )
    .unwrap();
    h.call("saveHtml", json!({ "html": "x", "source": "docs/a.md" }))
        .unwrap();
    assert_eq!(
        h.files.announced.lock().unwrap().clone(),
        [
            ("w-1".to_owned(), h.source().with_extension("pdf")),
            ("w-1".to_owned(), h.source().with_extension("html")),
        ]
    );
    let cancelled = cancelled_in(
        &h,
        "saveHtml",
        json!({ "html": "x", "source": "docs/a.md" }),
        "w-1",
    );
    assert_eq!(cancelled, (json!({ "outcome": "cancelled" }), true));
    assert_eq!(
        h.files.announced.lock().unwrap().len(),
        2,
        "nothing written"
    );
}

#[test]
fn switched_off_it_leaves_no_method_behind() {
    let mut h = Harness::start();
    assert!(h.call("setup.get", Value::Null).is_ok());
    h.running = None;
    for method in [
        "setup.get",
        "setup.set",
        "capabilities",
        "saveHtml",
        "savePdf",
        "print",
        "cancel",
        "open",
        "reveal",
    ] {
        assert_eq!(
            h.call(method, Value::Null).unwrap_err().kind,
            ErrorKind::Unavailable,
            "{method}"
        );
    }
}

/// A finished page of ~20 MB (pictures inlined as data URIs) through the plugin call's JSON edge.
/// Run: `cargo test -p permark-plugin-export large -- --ignored --nocapture`.
#[test]
#[ignore = "timing measurement"]
fn a_large_page_crosses_the_call_edge() {
    let h = Harness::start();
    let html = format!(
        "<img src=\"data:image/png;base64,{}\">",
        "A".repeat(20 << 20)
    );
    let started = std::time::Instant::now();
    let args = json!({ "html": html, "source": "docs/a.md", "setup": PageSetup::default() });
    let built = started.elapsed();
    let called = std::time::Instant::now();
    h.call("savePdf", args).unwrap();
    println!(
        "20 MB page: args built in {built:?}, call in {:?}",
        called.elapsed()
    );
}
