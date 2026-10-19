//! Opening from outside: command line parse, `OpenRequest` (forwarded by a second launch | macOS open events),
//! where each path goes (pure routing over the registry), `--new-markdown` file creation.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::VaultPath;
use crate::workspace::{WorkspaceRef, WorkspaceRegistry};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenFlags {
    pub new_window: bool,
    pub notes: bool,
    pub new_markdown_in: Option<PathBuf>,
}

/// What a launch asks the running app to open. Paths absolute (made so against `cwd`), not yet checked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    pub paths: Vec<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub flags: OpenFlags,
}

impl OpenRequest {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.flags.notes && self.flags.new_markdown_in.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAck {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cli {
    Open(OpenRequest),
    /// `permark mcp` (agent server bridge).
    Mcp,
    /// `permark context`.
    Context,
    Help,
    Version,
    /// Bad arguments: message for stderr, exit 2.
    Usage(String),
}

pub const USAGE: &str = "\
Usage: permark [options] [path...]
       permark mcp | context

Opens folders as projects and files on their own.

Options:
  --new-window           open in a new window
  --notes                open the Notes
  --new-markdown <dir>   create untitled.md in <dir> and open it
  -h, --help             show this help
  -V, --version          show the version";

/// A drive root from a file manager entry arrives as `C:"` (`"C:\"` quoting): back to `C:\`.
fn unquote_drive_root(arg: OsString) -> OsString {
    let text = arg.to_string_lossy();
    match text.as_bytes() {
        [letter, b':', b'"'] if letter.is_ascii_alphabetic() => {
            OsString::from(format!("{}:\\", char::from(*letter)))
        }
        _ => arg,
    }
}

fn absolute(arg: OsString, cwd: Option<&Path>) -> Option<PathBuf> {
    let path = PathBuf::from(unquote_drive_root(arg));
    if path.is_absolute() {
        Some(path)
    } else {
        cwd.map(|cwd| cwd.join(path))
    }
}

/// Parses `argv` (without the program name). Pure.
#[must_use]
pub fn parse_args(args: &[OsString], cwd: Option<&Path>) -> Cli {
    let mut iter = args.iter().cloned();
    let mut request = OpenRequest {
        cwd: cwd.map(Path::to_path_buf),
        ..OpenRequest::default()
    };
    let mut only_paths = false;
    let mut first = true;
    while let Some(arg) = iter.next() {
        let text = arg.to_string_lossy().into_owned();
        if first && !only_paths {
            first = false;
            match text.as_str() {
                "mcp" => return Cli::Mcp,
                "context" => return Cli::Context,
                _ => {}
            }
        }
        if only_paths || !text.starts_with('-') || text == "-" {
            match absolute(arg, cwd) {
                Some(path) => request.paths.push(path),
                None => return Cli::Usage(format!("cannot resolve {text}: no current folder")),
            }
            continue;
        }
        match text.as_str() {
            "--" => only_paths = true,
            "--new-window" => request.flags.new_window = true,
            "--notes" => request.flags.notes = true,
            "--new-markdown" => {
                let Some(folder) = iter.next().and_then(|a| absolute(a, cwd)) else {
                    return Cli::Usage("--new-markdown needs a folder".to_owned());
                };
                request.flags.new_markdown_in = Some(folder);
            }
            "-h" | "--help" => return Cli::Help,
            "-V" | "--version" => return Cli::Version,
            // macOS (older) adds a process serial number argument when Finder starts the app
            other if other.starts_with("-psn_") => {}
            other => return Cli::Usage(format!("unknown option {other}")),
        }
    }
    Cli::Open(request)
}

/// Creates `untitled.md` in `folder` (else `untitled-2.md`, ... free name, case-insensitive) holding `# untitled\n`.
///
/// # Errors
/// Folder missing | not writable | no free name.
pub fn create_untitled_markdown(folder: &Path) -> io::Result<PathBuf> {
    let taken: BTreeSet<String> = std::fs::read_dir(folder)?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .map(|n| n.to_lowercase())
        .collect();
    let name = std::iter::once("untitled.md".to_owned())
        .chain((2..10_000).map(|n| format!("untitled-{n}.md")))
        .find(|name| !taken.contains(name))
        .ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "no free untitled name"))?;
    let path = folder.join(&name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let title = name.trim_end_matches(".md").replace(['-', '_'], " ");
    if let Err(error) = file.write_all(format!("# {title}\n").as_bytes()) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

/// Where one outside path goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Already shown: focus that window (document = file inside the project).
    Focus {
        window: String,
        document: Option<VaultPath>,
    },
    /// Open in this window (it is on Welcome).
    OpenIn {
        window: String,
        document: Option<VaultPath>,
    },
    NewWindow {
        document: Option<VaultPath>,
    },
}

/// Decides where `reference` goes. `claimed`: Welcome windows already used by earlier paths of the same request.
/// A file inside an open project -> that project (with the file as document).
#[must_use]
pub fn route(
    registry: &WorkspaceRegistry,
    reference: &WorkspaceRef,
    force_new: bool,
    claimed: &mut BTreeSet<String>,
) -> Route {
    if !force_new {
        let shown = registry.find(reference).map(|id| (id, None));
        let inside = || match reference {
            WorkspaceRef::SingleFile(path) => registry
                .find_containing(path)
                .map(|(id, rel)| (id, Some(rel))),
            WorkspaceRef::Project(_) | WorkspaceRef::Notes => None,
        };
        if let Some((id, document)) = shown.or_else(inside) {
            if let Some(window) = registry.windows_of(id).into_iter().next() {
                return Route::Focus { window, document };
            }
        }
        if let Some(window) = registry
            .welcome_windows()
            .into_iter()
            .find(|w| !claimed.contains(w))
        {
            claimed.insert(window.clone());
            return Route::OpenIn {
                window,
                document: None,
            };
        }
    }
    Route::NewWindow { document: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn cwd() -> PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn paths_flags_and_subcommands() {
        let Cli::Open(r) = parse_args(
            &os(&["docs", "--new-window", "my notes/a.md"]),
            Some(&cwd()),
        ) else {
            panic!();
        };
        assert_eq!(r.paths, [cwd().join("docs"), cwd().join("my notes/a.md")]);
        assert!(r.flags.new_window);
        assert_eq!(parse_args(&os(&["mcp"]), None), Cli::Mcp);
        assert_eq!(parse_args(&os(&["context", "x"]), None), Cli::Context);
        // a folder named mcp after another path is a path
        let Cli::Open(r) = parse_args(&os(&["a", "mcp"]), Some(&cwd())) else {
            panic!()
        };
        assert_eq!(r.paths.len(), 2);
        assert_eq!(parse_args(&os(&["--help"]), None), Cli::Help);
        assert_eq!(parse_args(&os(&["-V"]), None), Cli::Version);
        assert!(matches!(parse_args(&os(&[]), None), Cli::Open(r) if r.is_empty()));
    }

    #[test]
    fn bad_arguments_are_usage_errors() {
        assert!(matches!(
            parse_args(&os(&["--frobnicate"]), None),
            Cli::Usage(_)
        ));
        assert!(matches!(
            parse_args(&os(&["--new-markdown"]), None),
            Cli::Usage(_)
        ));
        assert!(matches!(
            parse_args(&os(&["relative"]), None),
            Cli::Usage(_)
        ));
    }

    #[test]
    fn double_dash_and_dash_names_are_paths() {
        let Cli::Open(r) = parse_args(&os(&["--", "--notes", "-x.md"]), Some(&cwd())) else {
            panic!()
        };
        assert_eq!(r.paths, [cwd().join("--notes"), cwd().join("-x.md")]);
        assert!(!r.flags.notes);
        let Cli::Open(r) = parse_args(&os(&["-psn_0_12345", "--notes"]), Some(&cwd())) else {
            panic!()
        };
        assert!(r.flags.notes && r.paths.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn quoted_drive_root_and_unc_and_unicode() {
        let Cli::Open(r) = parse_args(
            &os(&["C:\"", r"\\server\share\x", "文档 ação"]),
            Some(Path::new(r"D:\w")),
        ) else {
            panic!()
        };
        assert_eq!(
            r.paths,
            [
                PathBuf::from(r"C:\"),
                PathBuf::from(r"\\server\share\x"),
                PathBuf::from(r"D:\w\文档 ação")
            ]
        );
    }

    #[test]
    fn new_markdown_folder_and_wire_format() {
        let Cli::Open(r) = parse_args(&os(&["--new-markdown", "here"]), Some(&cwd())) else {
            panic!()
        };
        assert_eq!(r.flags.new_markdown_in, Some(cwd().join("here")));
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<OpenRequest>(&json).unwrap(), r);
    }

    #[test]
    fn untitled_names_count_up_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let a = create_untitled_markdown(dir.path()).unwrap();
        assert_eq!(a.file_name().unwrap(), "untitled.md");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "# untitled\n");
        std::fs::write(dir.path().join("UNTITLED-2.md"), "x").unwrap();
        let b = create_untitled_markdown(dir.path()).unwrap();
        assert_eq!(b.file_name().unwrap(), "untitled-3.md");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "# untitled 3\n");
        assert!(create_untitled_markdown(&dir.path().join("missing")).is_err());
    }

    fn registry() -> (tempfile::TempDir, Arc<WorkspaceRegistry>) {
        use crate::conditions::Distribution;
        use crate::events::NoEvents;
        use crate::plugins::PluginRegistry;
        use crate::settings::AppSettings;
        use crate::state::AppDirs;
        use crate::workspace::CoreEnv;
        let base = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::empty()),
            Arc::new(NoEvents),
            Distribution::Direct,
            AppSettings::default(),
        );
        (base, Arc::new(WorkspaceRegistry::new(Arc::new(env))))
    }

    #[test]
    fn routing_focuses_shown_then_uses_welcome_then_new_window() {
        let (_b, registry) = registry();
        registry.add_window("main");
        registry.add_window("w-1");
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("docs")).unwrap();
        let file = project.path().join("docs").join("a.md");
        std::fs::write(&file, "x").unwrap();
        let reference = WorkspaceRef::classify(project.path()).unwrap();
        let mut claimed = BTreeSet::new();
        assert_eq!(
            route(&registry, &reference, false, &mut claimed),
            Route::OpenIn {
                window: "main".to_owned(),
                document: None
            }
        );
        registry.open("main", &reference, None).unwrap();
        // same project again -> focus
        assert_eq!(
            route(&registry, &reference, false, &mut BTreeSet::new()),
            Route::Focus {
                window: "main".to_owned(),
                document: None
            }
        );
        // a file inside it -> focus with document
        let inner = WorkspaceRef::classify(&file).unwrap();
        assert_eq!(
            route(&registry, &inner, false, &mut BTreeSet::new()),
            Route::Focus {
                window: "main".to_owned(),
                document: Some(VaultPath::parse("docs/a.md").unwrap())
            }
        );
        // two unrelated paths in one request: first takes the free welcome window, second a new window
        let (x, y) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut claimed = BTreeSet::new();
        let first = route(
            &registry,
            &WorkspaceRef::classify(x.path()).unwrap(),
            false,
            &mut claimed,
        );
        let second = route(
            &registry,
            &WorkspaceRef::classify(y.path()).unwrap(),
            false,
            &mut claimed,
        );
        assert_eq!(
            first,
            Route::OpenIn {
                window: "w-1".to_owned(),
                document: None
            }
        );
        assert_eq!(second, Route::NewWindow { document: None });
        // forced new window
        assert_eq!(
            route(&registry, &reference, true, &mut BTreeSet::new()),
            Route::NewWindow { document: None }
        );
    }
}
