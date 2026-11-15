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

/// A workspace with one document, `docs/a.md`, under `root`.
struct Files {
    root: PathBuf,
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
    },
    Open(String),
    Reveal(String),
}

/// A printer that records; a PDF waits for `release` when one is given (a render in progress).
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
        if let Some(started) = self.started.lock().unwrap().take() {
            started.send(()).unwrap();
        }
        if let Some(release) = self.release.lock().unwrap().take() {
            release.recv().unwrap();
        }
        if cancel.load(Ordering::SeqCst) {
            return Ok(Saved::Cancelled);
        }
        Ok(Saved::Saved {
            path: origin.source.with_extension("pdf"),
        })
    }

    fn save_html(&self, html: &str, origin: &Origin) -> Result<Saved, PrintError> {
        self.asked.lock().unwrap().push(Asked::Html {
            html: html.to_owned(),
            origin: origin.clone(),
        });
        Ok(Saved::Cancelled)
    }

    fn print(&self, html: String, setup: &PageSetup) -> Result<(), PrintError> {
        self.asked.lock().unwrap().push(Asked::Print {
            html,
            setup: *setup,
        });
        Ok(())
    }

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

struct Harness {
    rpc: Arc<RpcTable>,
    printer: Arc<Printer>,
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
        let leases = vec![
            services.provide(Arc::new(HtmlPrinterApi(
                Arc::clone(&printer) as Arc<dyn HtmlPrinter>
            ))),
            services.provide(Arc::new(WindowFilesApi(Arc::new(Files {
                root: root.path().to_path_buf(),
            })))),
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
        json!({ "outcome": "cancelled" })
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
            Asked::Print {
                html: "<p>p</p>".into(),
                setup: PageSetup::default()
            },
            Asked::Open("w-1".into()),
            Asked::Reveal("w-1".into()),
        ]
    );
    assert_eq!(
        h.call("capabilities", Value::Null).unwrap(),
        json!({ "pdf": true, "print": true, "pageNumbers": true })
    );
}

/// A PDF of window `w-1` in progress while `canceller` asks to cancel: what the call answers.
fn pdf_cancelled_by(canceller: &str) -> Value {
    let h = Harness::start();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    *h.printer.release.lock().unwrap() = Some(release_rx);
    *h.printer.started.lock().unwrap() = Some(started_tx);
    let rpc = Arc::clone(&h.rpc);
    let worker = std::thread::spawn(move || {
        call(
            &rpc,
            "w-1",
            "savePdf",
            json!({ "html": "x", "source": "docs/a.md", "setup": PageSetup::default() }),
        )
    });
    started_rx.recv().unwrap();
    call(&h.rpc, canceller, "cancel", Value::Null).unwrap();
    release_tx.send(()).unwrap();
    worker.join().unwrap().unwrap()
}

#[test]
fn cancel_drops_the_pdf_of_its_own_window_only() {
    assert_eq!(pdf_cancelled_by("w-1"), json!({ "outcome": "cancelled" }));
    assert_eq!(pdf_cancelled_by("w-2")["outcome"], "saved");
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
