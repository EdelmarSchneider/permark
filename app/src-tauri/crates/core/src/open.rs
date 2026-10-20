//! Opening from outside: command line parse, `OpenRequest` (forwarded by a second launch | macOS open events),
//! where each path goes (pure routing over the registry), `--new-markdown` file creation.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::paths::VaultPath;
use crate::settings::OpenIn;
use crate::workspace::{Place, WorkspaceRef, WorkspaceRegistry};

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
    /// `permark mcp` (agent server bridge); `--structured`: answers carry `structuredContent` too.
    Mcp {
        structured: bool,
    },
    /// `permark context`.
    Context,
    Help,
    Version,
    /// Bad arguments: message for stderr, exit 2.
    Usage(String),
}

pub const USAGE: &str = "\
Usage: permark [options] [path...]
       permark mcp [--structured] | context

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
                "mcp" => {
                    return Cli::Mcp {
                        structured: iter.any(|a| a == "--structured"),
                    }
                }
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

/// Where an open from the UI goes (the frontend turns the `open_in` setting into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OpenPlacement {
    /// A new tab in this window.
    Tab,
    /// Replaces the active tab (its unsaved prompt ran first, in the frontend).
    ThisTab,
    NewWindow,
}

/// What an open from the UI in `view` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placed {
    /// Open already (one place): that window + tab forward; `document` = the file inside the project.
    Focus {
        place: Place,
        document: Option<VaultPath>,
    },
    /// Opens in the view's own tab.
    ThisTab,
    /// A new tab in this window.
    Tab {
        window: String,
    },
    NewWindow,
}

/// Decides an open from the UI: open already anywhere (it, or a project tab holding the file) -> focus, whatever the
/// placement; else `placement`, where this tab on a view without a tab (the Welcome) is a new tab (it adopts the
/// view). `None`: unknown view.
#[must_use]
pub fn place_open(
    registry: &WorkspaceRegistry,
    view: &str,
    reference: &WorkspaceRef,
    placement: OpenPlacement,
) -> Option<Placed> {
    if let Some(place) = registry.find(reference) {
        return Some(Placed::Focus {
            place,
            document: None,
        });
    }
    if let WorkspaceRef::SingleFile(path) = reference {
        if let Some((place, document)) = registry.find_containing(path) {
            return Some(Placed::Focus {
                place,
                document: Some(document),
            });
        }
    }
    let window = registry.window_of(view)?;
    Some(match placement {
        OpenPlacement::NewWindow => Placed::NewWindow,
        OpenPlacement::ThisTab if registry.place_of_view(view).is_some() => Placed::ThisTab,
        OpenPlacement::ThisTab | OpenPlacement::Tab => Placed::Tab { window },
    })
}

/// Where one outside path goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Already open (one place): focus that window + tab; `document` = the file inside the project.
    Focus {
        place: Place,
        document: Option<VaultPath>,
    },
    /// A new tab in this work window.
    Tab { window: String },
    /// A new work window (later paths of the request join it: `joined`).
    NewWindow,
}

/// Decides where `reference` goes. Open already (it, or a project tab holding the file) -> focus, whatever the
/// flags; `new_window` | `open_in = window` -> the window this request made (`joined`), else a new one; else a tab
/// in the work window focused last (none -> a new window).
#[must_use]
pub fn route(
    registry: &WorkspaceRegistry,
    reference: &WorkspaceRef,
    new_window: bool,
    open_in: OpenIn,
    joined: Option<&str>,
) -> Route {
    if let Some(place) = registry.find(reference) {
        return Route::Focus {
            place,
            document: None,
        };
    }
    if let WorkspaceRef::SingleFile(path) = reference {
        if let Some((place, document)) = registry.find_containing(path) {
            return Route::Focus {
                place,
                document: Some(document),
            };
        }
    }
    if let Some(window) = joined {
        return Route::Tab {
            window: window.to_owned(),
        };
    }
    if new_window || open_in == OpenIn::NewWindow {
        return Route::NewWindow;
    }
    registry
        .last_focused_work()
        .map_or(Route::NewWindow, |window| Route::Tab { window })
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
        assert_eq!(
            parse_args(&os(&["mcp"]), None),
            Cli::Mcp { structured: false }
        );
        assert_eq!(
            parse_args(&os(&["mcp", "--structured"]), None),
            Cli::Mcp { structured: true }
        );
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

    /// A tab open in `window` (the whole open: reserve, prepare, finish).
    fn open_tab(registry: &WorkspaceRegistry, window: &str, reference: &WorkspaceRef) -> Place {
        match registry.open_tab(window, reference, None).unwrap() {
            crate::workspace::TabOpen::Opening { tab, outcome, .. } => {
                for (ticket, reference) in outcome.loads {
                    registry.load(ticket, &reference).unwrap();
                }
                Place {
                    window: window.to_owned(),
                    tab,
                }
            }
            crate::workspace::TabOpen::Existing(place) => place,
        }
    }

    /// Placement table: `open_in` x this view on Welcome | in a tab x open here | elsewhere | file inside an open
    /// project.
    #[test]
    fn placement_of_opens_from_the_ui() {
        use crate::workspace::WindowKind;
        let (_b, registry) = registry();
        for window in ["main", "w-1"] {
            registry.add_window(window, WindowKind::Work);
        }
        let _ = registry.show_welcome_view("main");
        let welcome = registry.shown_view("main").unwrap();
        let (here, elsewhere, fresh) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        std::fs::write(elsewhere.path().join("in.md"), "x").unwrap();
        let fresh = WorkspaceRef::classify(fresh.path()).unwrap();
        let all = [
            OpenPlacement::Tab,
            OpenPlacement::ThisTab,
            OpenPlacement::NewWindow,
        ];
        // the Welcome view: this tab = a new tab (it becomes the window's first)
        let on_welcome: Vec<Placed> = all
            .iter()
            .map(|p| place_open(&registry, &welcome, &fresh, *p).unwrap())
            .collect();
        let tab = Placed::Tab {
            window: "main".into(),
        };
        assert_eq!(on_welcome, [tab.clone(), tab.clone(), Placed::NewWindow]);
        // a view in a tab
        let here_place = open_tab(
            &registry,
            "main",
            &WorkspaceRef::classify(here.path()).unwrap(),
        );
        let view = registry.view_of_tab(here_place.tab).unwrap();
        let in_tab: Vec<Placed> = all
            .iter()
            .map(|p| place_open(&registry, &view, &fresh, *p).unwrap())
            .collect();
        assert_eq!(in_tab, [tab, Placed::ThisTab, Placed::NewWindow]);
        // open here | in another window | a file inside an open project: focus, whatever the placement
        let there = open_tab(
            &registry,
            "w-1",
            &WorkspaceRef::classify(elsewhere.path()).unwrap(),
        );
        for placement in all {
            assert_eq!(
                place_open(
                    &registry,
                    &view,
                    &WorkspaceRef::classify(here.path()).unwrap(),
                    placement
                ),
                Some(Placed::Focus {
                    place: here_place.clone(),
                    document: None
                })
            );
            assert_eq!(
                place_open(
                    &registry,
                    &view,
                    &WorkspaceRef::classify(elsewhere.path()).unwrap(),
                    placement
                ),
                Some(Placed::Focus {
                    place: there.clone(),
                    document: None
                })
            );
            assert_eq!(
                place_open(
                    &registry,
                    &view,
                    &WorkspaceRef::classify(&elsewhere.path().join("in.md")).unwrap(),
                    placement
                ),
                Some(Placed::Focus {
                    place: there.clone(),
                    document: Some(VaultPath::parse("in.md").unwrap())
                })
            );
        }
        assert_eq!(
            place_open(&registry, "v-nope", &fresh, OpenPlacement::Tab),
            None
        );
        // wire form
        let p: OpenPlacement = serde_json::from_str("\"thisTab\"").unwrap();
        assert_eq!(p, OpenPlacement::ThisTab);
        for old in ["\"ask\"", "\"thisWindow\"", "\"elsewhere\""] {
            assert!(serde_json::from_str::<OpenPlacement>(old).is_err(), "{old}");
        }
    }

    #[test]
    fn routing_focuses_what_is_open_else_the_last_focused_window_else_a_new_one() {
        use crate::workspace::WindowKind;
        let (_b, registry) = registry();
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("docs")).unwrap();
        let file = project.path().join("docs").join("a.md");
        std::fs::write(&file, "x").unwrap();
        let reference = WorkspaceRef::classify(project.path()).unwrap();
        // no window at all -> new window
        assert_eq!(
            route(&registry, &reference, false, OpenIn::Tab, None),
            Route::NewWindow
        );
        registry.add_window("main", WindowKind::Work);
        registry.add_window("w-1", WindowKind::Work);
        registry.focused("main", 10);
        registry.focused("w-1", 20);
        assert_eq!(
            route(&registry, &reference, false, OpenIn::Tab, None),
            Route::Tab {
                window: "w-1".to_owned()
            }
        );
        let place = open_tab(&registry, "main", &reference);
        // open -> focus, whatever the flags
        for (new_window, open_in) in [
            (false, OpenIn::Tab),
            (true, OpenIn::Tab),
            (false, OpenIn::NewWindow),
        ] {
            assert_eq!(
                route(&registry, &reference, new_window, open_in, Some("w-1")),
                Route::Focus {
                    place: place.clone(),
                    document: None
                }
            );
        }
        // a file inside the open project -> that tab, file as document
        let inner = WorkspaceRef::classify(&file).unwrap();
        assert_eq!(
            route(&registry, &inner, false, OpenIn::Tab, None),
            Route::Focus {
                place,
                document: Some(VaultPath::parse("docs/a.md").unwrap())
            }
        );
        let other = tempfile::tempdir().unwrap();
        let other = WorkspaceRef::classify(other.path()).unwrap();
        assert_eq!(
            route(&registry, &other, true, OpenIn::Tab, None),
            Route::NewWindow
        );
        assert_eq!(
            route(&registry, &other, false, OpenIn::NewWindow, None),
            Route::NewWindow
        );
        // later paths of one request join the window it made
        assert_eq!(
            route(&registry, &other, true, OpenIn::Tab, Some("w-9")),
            Route::Tab {
                window: "w-9".to_owned()
            }
        );
        // a window asking about its close is never chosen
        registry.focused("main", 30);
        let open_dir = tempfile::tempdir().unwrap();
        let unsaved_target = WorkspaceRef::classify(open_dir.path()).unwrap();
        let _ = registry.request_close("main");
        assert_eq!(
            route(&registry, &unsaved_target, false, OpenIn::Tab, None),
            Route::Tab {
                window: "w-1".to_owned()
            },
            "main closed (nothing unsaved): w-1 is the one left open"
        );
    }
}
