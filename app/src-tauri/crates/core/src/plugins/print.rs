//! Turning a finished HTML page into a PDF | a standalone HTML file | a printout, for an App-scope plugin (export).
//! The shell renders the page in the system web view and owns everything that touches the person's disk outside the
//! open folder: the Save dialog, the checks on the chosen path, the atomic write, opening the written file. The path
//! of a written file never comes from a web page.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::fsx::{self, SameFile};

/// Paper size of a PDF | printout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PageSize {
    #[default]
    A4,
    A5,
    A3,
    /// US Letter (8.5 x 11 in).
    Letter,
    /// US Legal (8.5 x 14 in).
    Legal,
    /// 11 x 17 in.
    Tabloid,
}

impl PageSize {
    /// Width and height in portrait, millimeters.
    #[must_use]
    pub const fn portrait_mm(self) -> (f64, f64) {
        match self {
            Self::A4 => (210.0, 297.0),
            Self::A5 => (148.0, 210.0),
            Self::A3 => (297.0, 420.0),
            Self::Letter => (215.9, 279.4),
            Self::Legal => (215.9, 355.6),
            Self::Tabloid => (279.4, 431.8),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Margins {
    Narrow,
    #[default]
    Normal,
    Wide,
}

impl Margins {
    /// The margin on every side, millimeters.
    #[must_use]
    pub const fn mm(self) -> f64 {
        match self {
            Self::Narrow => 10.0,
            Self::Normal => 20.0,
            Self::Wide => 30.0,
        }
    }
}

/// How the pages of a PDF | printout are laid out (remembered app-wide by the plugin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PageSetup {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margins: Margins,
    /// "n / total" at the bottom of each page (only where [`PrintCapabilities::page_numbers`]).
    pub page_numbers: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            size: PageSize::A4,
            orientation: Orientation::Portrait,
            margins: Margins::Normal,
            page_numbers: true,
        }
    }
}

impl PageSetup {
    /// The page box this setup gives: the one source of the numbers the page's CSS (`@page`) and the native print
    /// settings both use.
    #[must_use]
    pub fn page(&self) -> PageBox {
        let (w, h) = self.size.portrait_mm();
        let (width_mm, height_mm) = match self.orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        };
        PageBox {
            width_mm,
            height_mm,
            margin_mm: self.margins.mm(),
        }
    }
}

/// A page as laid out (orientation applied), millimeters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PageBox {
    pub width_mm: f64,
    pub height_mm: f64,
    pub margin_mm: f64,
}

/// What the system web view of this build can do.
#[allow(clippy::struct_excessive_bools)] // independent facts of one web view, not a state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrintCapabilities {
    /// PDF and print work at all.
    pub pdf: bool,
    pub print: bool,
    /// The page's CSS page-number boxes (`@page { @bottom-center }`) show in the output.
    pub page_numbers: bool,
    /// The paper's size and margins are set natively (`WebKit`: it draws inside its own margins, and lays the page
    /// out for the paper less the page's `@page` margins): the page's CSS names the same margins and draws no margin
    /// boxes.
    pub native_margins: bool,
    /// Running as a snap without its `cups` connection: the print dialog lists no printers until the CUPS snap is
    /// installed (the snap's `cups` plug connects only to that snap, never to the system's own CUPS).
    pub snap_needs_cups: bool,
}

/// What the person is writing: picks the dialog's start folder, its default name and its filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Pdf,
    Html,
}

impl OutputKind {
    #[must_use]
    pub const fn ext(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Html => "html",
        }
    }
}

/// The document an output is made from: the window that asked (the dialog sits on it, its last written file is
/// remembered for it) and the source on disk (checked inside that window's workspace by the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub window: String,
    pub source: PathBuf,
}

impl Origin {
    /// The name the dialog offers: the source's base name + the output's extension.
    #[must_use]
    pub fn suggested_name(&self, kind: OutputKind) -> String {
        let stem = self.source.file_stem().map_or_else(
            || "document".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        format!("{stem}.{}", kind.ext())
    }

    /// The folder the dialog starts in: the source's.
    #[must_use]
    pub fn folder(&self) -> Option<&Path> {
        self.source.parent()
    }
}

/// How an export ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "outcome")]
#[ts(export)]
pub enum Saved {
    /// Written (absolute path, for the notice).
    Saved {
        #[ts(type = "string")]
        path: PathBuf,
    },
    /// The person closed the dialog | cancelled before the write: nothing written.
    Cancelled,
}

/// Why an output was not made.
#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    #[error("this system's web view cannot print")]
    Unsupported,
    #[error("the chosen file is not a usable place: {0}")]
    BadTarget(String),
    #[error("{0} already exists and was not chosen to be replaced")]
    Taken(PathBuf),
    #[error("the chosen file is the document itself")]
    IsSource,
    #[error("nothing written yet")]
    NothingWritten,
    #[error("{0} is gone")]
    Gone(PathBuf),
    #[error("the system did not open {path}: {detail}")]
    Opener { path: PathBuf, detail: String },
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the web view failed: {0}")]
    Render(String),
}

impl From<PrintError> for IpcError {
    fn from(error: PrintError) -> Self {
        match error {
            PrintError::Unsupported => IpcError::new(ErrorKind::Unsupported).subject("print"),
            PrintError::BadTarget(path) => IpcError::new(ErrorKind::InvalidPath).subject(path),
            PrintError::Taken(path) => IpcError::new(ErrorKind::Conflict)
                .subject(path.display().to_string())
                .detail("exists"),
            PrintError::IsSource => IpcError::new(ErrorKind::Conflict).detail("source"),
            PrintError::NothingWritten => IpcError::new(ErrorKind::NotFound).subject("output"),
            PrintError::Gone(path) => {
                IpcError::new(ErrorKind::NotFound).subject(path.display().to_string())
            }
            PrintError::Opener { path, detail } => IpcError::new(ErrorKind::Io)
                .subject(path.display().to_string())
                .detail(detail),
            PrintError::Write { path, source } => IpcError::io(path.display().to_string(), &source),
            PrintError::Render(detail) => IpcError::new(ErrorKind::Internal)
                .subject("print")
                .detail(detail),
        }
    }
}

/// Renders and writes outputs; implemented by the shell (system web view, Save dialog).
pub trait HtmlPrinter: Send + Sync {
    /// What this system can do.
    fn capabilities(&self) -> PrintCapabilities;

    /// Asks where to save (Save dialog on the window), renders `html` as pages of `setup`, writes the PDF there.
    /// `cancel` set while the web view renders: the pages are dropped, nothing written (the render itself cannot stop).
    ///
    /// # Errors
    /// Unsupported | a bad | refused target | the web view failed | the write failed (nothing left behind).
    fn save_pdf(
        &self,
        html: String,
        setup: &PageSetup,
        origin: &Origin,
        cancel: &AtomicBool,
    ) -> Result<Saved, PrintError>;

    /// Asks where to save and writes `html` there as it is. `cancel` set by the time the dialog closes: nothing
    /// written.
    ///
    /// # Errors
    /// A bad | refused target | the write failed (nothing left behind).
    fn save_html(
        &self,
        html: &str,
        origin: &Origin,
        cancel: &AtomicBool,
    ) -> Result<Saved, PrintError>;

    /// Shows the system's print dialog for `html` laid out as `setup`, on `window` (the window that asked) where the
    /// system lets it. `cancel` set while the page loads: no dialog.
    ///
    /// # Errors
    /// Unsupported | the web view failed.
    fn print(
        &self,
        html: String,
        setup: &PageSetup,
        window: &str,
        cancel: &AtomicBool,
    ) -> Result<(), PrintError>;

    /// `window` is gone: the file last written for it is forgotten (Open | Show in folder no longer reach it).
    fn forget(&self, window: &str);

    /// Opens the file last written for `window` in the system's default program.
    ///
    /// # Errors
    /// Nothing written for it | the file is gone | the system refused.
    fn open(&self, window: &str) -> Result<(), PrintError>;

    /// Shows the file last written for `window` in the file manager.
    ///
    /// # Errors
    /// Nothing written for it | the file is gone | the system refused.
    fn reveal(&self, window: &str) -> Result<(), PrintError>;
}

/// The service plugins look up (`AppServices.services.get::<HtmlPrinterApi>()`).
#[derive(Clone)]
pub struct HtmlPrinterApi(pub Arc<dyn HtmlPrinter>);

/// The path an output is written to, from what the person chose in the Save dialog: absolute, the output's extension
/// (added when missing), an existing folder, never the source document (one file under two names counts).
///
/// # Errors
/// Relative | no folder | the extension had to be added and that file exists | the source.
pub fn output_path(chosen: &Path, kind: OutputKind, source: &Path) -> Result<PathBuf, PrintError> {
    let shown = || chosen.display().to_string();
    if !chosen.is_absolute() || chosen.file_name().is_none() {
        return Err(PrintError::BadTarget(shown()));
    }
    let has_ext = chosen
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(kind.ext()));
    let path = if has_ext {
        chosen.to_path_buf()
    } else {
        let mut name = chosen.as_os_str().to_owned();
        name.push(".");
        name.push(kind.ext());
        PathBuf::from(name)
    };
    if !path.parent().is_some_and(Path::is_dir) || path.is_dir() {
        return Err(PrintError::BadTarget(path.display().to_string()));
    }
    // an added extension names a file the dialog never asked to replace: refused rather than overwritten unasked
    if !has_ext && path.exists() {
        return Err(PrintError::Taken(path));
    }
    let same = match fsx::same_file(&path, source) {
        SameFile::Yes => true,
        SameFile::Other => false,
        // a volume without file ids: one name spelled two ways is still one file
        SameFile::Unknown => {
            path.exists()
                && crate::paths::fold(&path.to_string_lossy())
                    == crate::paths::fold(&source.to_string_lossy())
        }
    };
    if same {
        return Err(PrintError::IsSource);
    }
    Ok(path)
}

/// Writes an output's bytes at a checked path (temp file, then rename: a failure leaves no partial file and the old
/// file as it was).
///
/// # Errors
/// The write failed.
pub fn write_output(path: &Path, bytes: &[u8]) -> Result<(), PrintError> {
    fsx::write_atomic(path, bytes).map_err(|source| PrintError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_setup_is_a4_portrait_normal_with_numbers() {
        let setup = PageSetup::default();
        assert_eq!(setup.size, PageSize::A4);
        assert_eq!(setup.orientation, Orientation::Portrait);
        assert_eq!(setup.margins, Margins::Normal);
        assert!(setup.page_numbers);
    }

    #[test]
    fn landscape_swaps_the_sides_and_margins_come_from_one_table() {
        let setup = PageSetup {
            size: PageSize::A5,
            orientation: Orientation::Landscape,
            margins: Margins::Narrow,
            page_numbers: false,
        };
        assert_eq!(
            setup.page(),
            PageBox {
                width_mm: 210.0,
                height_mm: 148.0,
                margin_mm: 10.0
            }
        );
        let letter = PageSetup {
            size: PageSize::Letter,
            ..PageSetup::default()
        };
        assert_eq!(letter.page().width_mm, 215.9);
        assert!(Margins::Narrow.mm() < Margins::Normal.mm());
        assert!(Margins::Normal.mm() < Margins::Wide.mm());
    }

    #[test]
    fn setup_reads_and_writes_as_camel_case() {
        let setup = PageSetup {
            size: PageSize::Tabloid,
            orientation: Orientation::Landscape,
            margins: Margins::Wide,
            page_numbers: false,
        };
        let json = serde_json::to_value(setup).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "size": "tabloid", "orientation": "landscape", "margins": "wide", "pageNumbers": false
            })
        );
        assert_eq!(serde_json::from_value::<PageSetup>(json).unwrap(), setup);
        assert!(serde_json::from_value::<PageSetup>(serde_json::json!({
            "size": "b5", "orientation": "portrait", "margins": "normal", "pageNumbers": true
        }))
        .is_err());
    }

    #[test]
    fn the_dialog_offers_the_source_name_in_its_folder() {
        let origin = Origin {
            window: "w-1".into(),
            source: PathBuf::from("/docs/My notes.v2.md"),
        };
        assert_eq!(origin.suggested_name(OutputKind::Pdf), "My notes.v2.pdf");
        assert_eq!(origin.suggested_name(OutputKind::Html), "My notes.v2.html");
        assert_eq!(origin.folder(), Some(Path::new("/docs")));
    }

    #[test]
    fn the_extension_is_forced_and_kept_in_any_case() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.md");
        std::fs::write(&source, "# a").unwrap();
        let pdf = |name: &str| output_path(&dir.path().join(name), OutputKind::Pdf, &source);
        assert_eq!(pdf("a.pdf").unwrap(), dir.path().join("a.pdf"));
        assert_eq!(pdf("a.PDF").unwrap(), dir.path().join("a.PDF"));
        assert_eq!(pdf("a").unwrap(), dir.path().join("a.pdf"));
        assert_eq!(pdf("a.txt").unwrap(), dir.path().join("a.txt.pdf"));
        let html = output_path(&dir.path().join("a.htm"), OutputKind::Html, &source).unwrap();
        assert_eq!(html, dir.path().join("a.htm.html"));
    }

    #[test]
    fn an_added_extension_never_replaces_a_file_the_dialog_did_not_ask_about() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.md");
        std::fs::write(&source, "# a").unwrap();
        let taken = dir.path().join("report.pdf");
        std::fs::write(&taken, b"kept").unwrap();
        // the dialog confirmed "report"; "report.pdf" exists and was never offered for replacing
        let refused = output_path(&dir.path().join("report"), OutputKind::Pdf, &source);
        assert!(matches!(&refused, Err(PrintError::Taken(path)) if *path == taken));
        // its own reason over IPC: the window can name the file and ask for another name
        let error = IpcError::from(refused.unwrap_err());
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.detail.as_deref(), Some("exists"));
        assert_eq!(error.subject, Some(taken.display().to_string()));
        assert_eq!(std::fs::read(&taken).unwrap(), b"kept");
        // chosen with its extension: the dialog asked, the person agreed
        assert_eq!(
            output_path(&taken, OutputKind::Pdf, &source).unwrap(),
            taken
        );
    }

    #[test]
    fn a_relative_path_a_missing_folder_or_a_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.md");
        std::fs::write(&source, "# a").unwrap();
        let bad = |path: &Path| {
            matches!(
                output_path(path, OutputKind::Pdf, &source),
                Err(PrintError::BadTarget(_))
            )
        };
        assert!(bad(Path::new("out.pdf")));
        assert!(bad(&dir.path().join("missing").join("out.pdf")));
        std::fs::create_dir(dir.path().join("taken.pdf")).unwrap();
        assert!(bad(&dir.path().join("taken.pdf")));
    }

    #[test]
    fn the_source_itself_is_never_a_target() {
        let dir = tempfile::tempdir().unwrap();
        // an HTML document exported as HTML onto itself
        let source = dir.path().join("page.html");
        std::fs::write(&source, "<p>a</p>").unwrap();
        let refused = output_path(&source, OutputKind::Html, &source);
        assert!(matches!(refused, Err(PrintError::IsSource)));
        let kind = IpcError::from(refused.unwrap_err()).kind;
        assert_eq!(kind, ErrorKind::Conflict);
        // the same file under another spelling (case-insensitive systems), or through a hard link
        let link = dir.path().join("link.html");
        std::fs::hard_link(&source, &link).unwrap();
        assert!(matches!(
            output_path(&link, OutputKind::Html, &source),
            Err(PrintError::IsSource)
        ));
        if crate::paths::CASE_INSENSITIVE {
            assert!(matches!(
                output_path(&dir.path().join("PAGE.html"), OutputKind::Html, &source),
                Err(PrintError::IsSource)
            ));
        }
        // another existing file is replaced as the person chose
        let other = dir.path().join("other.html");
        std::fs::write(&other, "old").unwrap();
        assert_eq!(
            output_path(&other, OutputKind::Html, &source).unwrap(),
            other
        );
    }

    #[test]
    fn writing_replaces_the_file_whole_and_leaves_its_neighbors_alone() {
        let dir = tempfile::tempdir().unwrap();
        let neighbor = dir.path().join("a.md");
        std::fs::write(&neighbor, b"# a\r\nkept bytes\r\n").unwrap();
        let out = dir.path().join("a.pdf");
        std::fs::write(&out, b"older and longer content").unwrap();
        write_output(&out, b"%PDF-1.7").unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"%PDF-1.7");
        assert_eq!(std::fs::read(&neighbor).unwrap(), b"# a\r\nkept bytes\r\n");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "no temp file left: {names:?}");
    }
}
