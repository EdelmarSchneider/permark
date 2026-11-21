//! The person's editor: a command template such as `code -g {path}:{line}:{col}` that opens a file at a line.
//!
//! Rules (directives): the program and its arguments come from the template only; a document supplies a path and
//! numbers. The template is split into arguments (quotes respected) before any placeholder is filled, so the path is
//! always inside one argument, whatever it contains. No shell. The program is an absolute path or a bare name looked
//! up on `PATH` and the usual tool folders (`process::resolve_program`), never in the working folder, the project or
//! on a share. It starts only once the person allowed it (`process::ProgramApprovals`: a dialog names its full path),
//! detached, in the home folder.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::error::{ErrorKind, IpcError};
use crate::process::{self, ProgramApprovals, ProgramCall};

/// Longest template accepted.
pub const MAX_TEMPLATE: usize = 400;

/// What is wrong with a template (`code()` = a stable word for the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateProblem {
    TooLong,
    UnbalancedQuote,
    /// The program itself holds a placeholder.
    ProgramPlaceholder,
    UnknownPlaceholder,
    /// No `{path}`: the file would not be passed.
    NoPath,
}

impl TemplateProblem {
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::TooLong => "tooLong",
            Self::UnbalancedQuote => "unbalancedQuote",
            Self::ProgramPlaceholder => "programPlaceholder",
            Self::UnknownPlaceholder => "unknownPlaceholder",
            Self::NoPath => "noPath",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    #[error("no editor is set")]
    NotSet,
    #[error("the editor command is not valid: {}", .0.code())]
    Template(TemplateProblem),
    #[error("the program {0} was not found")]
    NotFound(String),
    #[error("the argument would start with a dash")]
    Dash,
    #[error("the editor could not be started: {0}")]
    Spawn(String),
    #[error("the person did not allow {0} to start")]
    NotAllowed(String),
}

impl From<EditorError> for IpcError {
    fn from(error: EditorError) -> Self {
        match error {
            EditorError::NotSet => IpcError::new(ErrorKind::EditorNotSet),
            EditorError::Template(problem) => {
                IpcError::new(ErrorKind::EditorFailed).detail(problem.code())
            }
            EditorError::NotFound(program) => IpcError::new(ErrorKind::EditorFailed)
                .subject(program)
                .detail("notFound"),
            EditorError::Dash => IpcError::new(ErrorKind::EditorFailed).detail("dash"),
            EditorError::Spawn(detail) => IpcError::new(ErrorKind::EditorFailed).detail(detail),
            EditorError::NotAllowed(program) => IpcError::new(ErrorKind::Denied).subject(program),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Path,
    Line,
    Col,
}

/// A parsed template: the program and the arguments, each made of text and placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTemplate {
    program: String,
    args: Vec<Vec<Piece>>,
}

impl EditorTemplate {
    /// `Ok(None)` for an empty template (no editor set).
    ///
    /// # Errors
    /// The problem with the template.
    pub fn parse(template: &str) -> Result<Option<Self>, TemplateProblem> {
        if template.len() > MAX_TEMPLATE {
            return Err(TemplateProblem::TooLong);
        }
        let words = split_args(template)?;
        let Some((program, rest)) = words.split_first() else {
            return Ok(None);
        };
        if program.contains('{')
            && pieces(program)?
                .iter()
                .any(|p| !matches!(p, Piece::Text(_)))
        {
            return Err(TemplateProblem::ProgramPlaceholder);
        }
        let args = rest
            .iter()
            .map(|arg| pieces(arg))
            .collect::<Result<Vec<_>, _>>()?;
        if !args.iter().flatten().any(|piece| *piece == Piece::Path) {
            return Err(TemplateProblem::NoPath);
        }
        Ok(Some(Self {
            program: program.clone(),
            args,
        }))
    }

    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// `program` (the template's program found) with the template's arguments as written, placeholders unfilled.
    fn call(&self, program: PathBuf) -> ProgramCall {
        let args = self
            .args
            .iter()
            .map(|pieces| {
                pieces
                    .iter()
                    .map(|piece| match piece {
                        Piece::Text(text) => text.as_str(),
                        Piece::Path => "{path}",
                        Piece::Line => "{line}",
                        Piece::Col => "{col}",
                    })
                    .collect()
            })
            .collect();
        ProgramCall {
            program,
            args,
            env: Vec::new(),
        }
    }

    /// The arguments with `path` (absolute), `line` and `col` (from 1) filled in.
    ///
    /// # Errors
    /// `Dash`: an argument that is the path alone would start with `-`.
    pub fn arguments(
        &self,
        path: &Path,
        line: u32,
        col: u32,
    ) -> Result<Vec<OsString>, EditorError> {
        self.args
            .iter()
            .map(|pieces| {
                let mut arg = OsString::new();
                for piece in pieces {
                    match piece {
                        Piece::Text(text) => arg.push(text),
                        Piece::Path => arg.push(path),
                        Piece::Line => arg.push(line.to_string()),
                        Piece::Col => arg.push(col.to_string()),
                    }
                }
                if matches!(pieces.as_slice(), [Piece::Path])
                    && arg.to_string_lossy().starts_with('-')
                {
                    return Err(EditorError::Dash);
                }
                Ok(arg)
            })
            .collect()
    }
}

/// Splits a template into arguments. Quotes group (and are dropped); an empty pair of quotes is an empty argument;
/// a backslash is plain text (Windows paths are written as they are).
fn split_args(template: &str) -> Result<Vec<String>, TemplateProblem> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for c in template.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(TemplateProblem::UnbalancedQuote);
    }
    if started {
        words.push(current);
    }
    Ok(words)
}

/// Cuts one argument into text and placeholders. Only `{name}` with letters, digits or `_` is a placeholder (known
/// names: `path`, `line`, `col`); any other brace is text.
fn pieces(arg: &str) -> Result<Vec<Piece>, TemplateProblem> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = arg;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let name = &after[..close];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            text.push_str(&rest[..open]);
            if !text.is_empty() {
                out.push(Piece::Text(std::mem::take(&mut text)));
            }
            out.push(match name {
                "path" => Piece::Path,
                "line" => Piece::Line,
                "col" => Piece::Col,
                _ => return Err(TemplateProblem::UnknownPlaceholder),
            });
            rest = &after[close + 1..];
        } else {
            text.push_str(&rest[..=open]);
            rest = after;
        }
    }
    text.push_str(rest);
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    Ok(out)
}

// ---- starting the program -----------------------------------------------------------------------------------------

/// The program `template` starts (found as every program the app starts: [`process::resolve_program`]) with the
/// template's words, placeholders unfilled: what the person allows ([`ProgramApprovals`]). `None`: no editor, an
/// invalid template, a program not found.
#[must_use]
pub fn program_call(template: &str) -> Option<ProgramCall> {
    let parsed = EditorTemplate::parse(template).ok()??;
    let program = process::resolve_program(OsStr::new(parsed.program()))?;
    Some(parsed.call(program))
}

/// Starts the editor on `path` (absolute, validated by the caller through the vault) at `line` | `col`: only a
/// program the person allowed (`approvals` asks about a new one: blocks while the dialog is up), detached with a
/// hidden console (a `.cmd` editor's console children open no window), in a neutral folder.
///
/// # Errors
/// Not set, invalid template, program not found (a relative | network path | one inside an open project is never
/// started), not allowed, start failure.
pub fn launch(
    template: &str,
    path: &Path,
    line: u32,
    col: u32,
    approvals: &ProgramApprovals,
) -> Result<(), EditorError> {
    launch_with(template, path, line, col, approvals, |program, args| {
        process::spawn_detached(
            program,
            args,
            Some(&neutral_dir()),
            process::Console::Hidden,
        )
    })
}

/// [`launch`] with the start itself given (`spawn`: program, filled arguments).
fn launch_with(
    template: &str,
    path: &Path,
    line: u32,
    col: u32,
    approvals: &ProgramApprovals,
    spawn: impl FnOnce(&Path, &[&OsStr]) -> std::io::Result<()>,
) -> Result<(), EditorError> {
    let parsed = EditorTemplate::parse(template)
        .map_err(EditorError::Template)?
        .ok_or(EditorError::NotSet)?;
    let program = process::resolve_program(OsStr::new(parsed.program()))
        .filter(|program| !approvals.in_project(program))
        .ok_or_else(|| EditorError::NotFound(parsed.program().to_owned()))?;
    let args = parsed.arguments(path, line.max(1), col.max(1))?;
    let call = parsed.call(program);
    if !approvals.allow(&call) {
        return Err(EditorError::NotAllowed(call.program.display().to_string()));
    }
    let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    spawn(&call.program, &args).map_err(|e| EditorError::Spawn(e.to_string()))
}

/// The folder the editor starts in: home, else the temporary folder. Never the project.
fn neutral_dir() -> PathBuf {
    dirs::home_dir()
        .filter(|h| h.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::plugins::ProgramConsent;
    use crate::state::{AppDirs, StateStore};

    fn parse(template: &str) -> EditorTemplate {
        EditorTemplate::parse(template).unwrap().unwrap()
    }

    fn args(template: &str, path: &str) -> Vec<String> {
        parse(template)
            .arguments(Path::new(path), 12, 3)
            .unwrap()
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn presets_fill_path_line_and_column() {
        assert_eq!(
            args("code -g {path}:{line}:{col}", "/p/a.rs"),
            ["-g", "/p/a.rs:12:3"]
        );
        assert_eq!(parse("code -g {path}:{line}:{col}").program(), "code");
        assert_eq!(args("zed {path}:{line}", "/p/a.rs"), ["/p/a.rs:12"]);
        assert_eq!(
            args("subl \"{path}\":{line}", "C:\\My Docs\\a.rs"),
            ["C:\\My Docs\\a.rs:12"]
        );
        assert_eq!(
            args("idea --line {line} {path}", "/p/a.rs"),
            ["--line", "12", "/p/a.rs"]
        );
    }

    #[test]
    fn the_path_is_one_argument_whatever_it_holds() {
        for hostile in [
            "/p/a b.rs",
            "/p/a;rm -rf.rs",
            "/p/$(x).rs",
            "/p/'q'.rs",
            "/p/\"d\".rs",
            "/p/&|<>.rs",
            "C:\\a b\\c.rs",
        ] {
            let got = args("code {path}", hostile);
            assert_eq!(got, [hostile.to_owned()]);
        }
        // spaces in the program path via quotes
        let t = parse("\"C:\\Program Files\\Code\\code.exe\" -g {path}");
        assert_eq!(t.program(), "C:\\Program Files\\Code\\code.exe");
        assert_eq!(parse("'my editor' {path}").program(), "my editor");
        // an empty pair of quotes is an empty argument
        assert_eq!(args("ed \"\" {path}", "/a"), ["", "/a"]);
    }

    #[test]
    fn bad_templates_are_named() {
        assert_eq!(EditorTemplate::parse("").unwrap(), None);
        assert_eq!(EditorTemplate::parse("   ").unwrap(), None);
        for (template, problem) in [
            ("code", TemplateProblem::NoPath),
            ("code {line}", TemplateProblem::NoPath),
            ("code \"{path}", TemplateProblem::UnbalancedQuote),
            ("{path} x", TemplateProblem::ProgramPlaceholder),
            ("co{line}de {path}", TemplateProblem::ProgramPlaceholder),
            ("code {file}", TemplateProblem::UnknownPlaceholder),
        ] {
            assert_eq!(
                EditorTemplate::parse(template).unwrap_err(),
                problem,
                "{template}"
            );
        }
        assert_eq!(
            EditorTemplate::parse(&format!("code {}", "x".repeat(MAX_TEMPLATE))).unwrap_err(),
            TemplateProblem::TooLong
        );
        // braces that are not placeholders are text
        assert_eq!(
            args("code {path} {} { x } {a-b}", "/p"),
            ["/p", "{}", "{", "x", "}", "{a-b}"]
        );
    }

    #[test]
    fn a_path_cannot_become_an_option() {
        let t = parse("code {path}");
        assert!(matches!(
            t.arguments(Path::new("-evil"), 1, 1),
            Err(EditorError::Dash)
        ));
        // joined with text it is not an option
        let t = parse("code --file={path}");
        assert!(t.arguments(Path::new("-evil"), 1, 1).is_ok());
    }

    /// Answers with `yes`; keeps the programs it was asked about.
    #[derive(Default)]
    struct Asked {
        yes: AtomicBool,
        asks: Mutex<Vec<PathBuf>>,
    }

    impl ProgramConsent for Asked {
        fn allow(&self, call: &ProgramCall) -> bool {
            self.asks.lock().unwrap().push(call.program.clone());
            self.yes.load(Ordering::SeqCst)
        }
    }

    impl Asked {
        fn count(&self) -> usize {
            self.asks.lock().unwrap().len()
        }
    }

    fn approvals(profile: &Path, asked: &Arc<Asked>) -> ProgramApprovals {
        let store = StateStore::app(&AppDirs {
            profile: profile.to_path_buf(),
            cache: profile.join("cache"),
            install: profile.join("install"),
        });
        let consent: Arc<dyn ProgramConsent> = Arc::clone(asked) as _;
        ProgramApprovals::of(&store, Some(consent))
    }

    /// A script that leaves `started.txt` beside itself when it runs.
    fn marking_editor(dir: &Path) -> PathBuf {
        #[cfg(windows)]
        let script = {
            let script = dir.join("edit.cmd");
            std::fs::write(&script, "@echo started> \"%~dp0started.txt\"\r\n").unwrap();
            script
        };
        #[cfg(unix)]
        let script = {
            use std::os::unix::fs::PermissionsExt;
            let script = dir.join("edit");
            std::fs::write(
                &script,
                "#!/bin/sh\necho started > \"$(dirname \"$0\")/started.txt\"\n",
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            script
        };
        script
    }

    fn template_of(program: &Path) -> String {
        format!("\"{}\" --goto {{path}}:{{line}}", program.display())
    }

    #[test]
    fn launch_reports_not_set_missing_and_bad_templates() {
        let profile = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        let gate = approvals(profile.path(), &asked);
        let file = Path::new("/p/a.rs");
        assert!(matches!(
            launch("", file, 1, 1, &gate),
            Err(EditorError::NotSet)
        ));
        assert!(matches!(
            launch("definitely-not-a-program-xyz {path}", file, 1, 1, &gate),
            Err(EditorError::NotFound(_))
        ));
        assert!(matches!(
            launch("code", file, 1, 1, &gate),
            Err(EditorError::Template(TemplateProblem::NoPath))
        ));
        let error = IpcError::from(EditorError::NotSet);
        assert_eq!(error.kind, ErrorKind::EditorNotSet);
        assert_eq!(asked.count(), 0);
    }

    /// A program on a share | named by a relative path is never looked up, never asked about, never started; also
    /// when the file is there (relative to the tests' folder: `src/lib.rs`).
    #[test]
    fn network_and_relative_programs_are_refused() {
        let profile = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        asked.yes.store(true, Ordering::SeqCst);
        let gate = approvals(profile.path(), &asked);
        let file = Path::new("/p/a.rs");
        assert!(Path::new("src").join("lib.rs").is_file());
        for template in [
            r"\\host\share\code.exe {path}",
            "//host/share/code {path}",
            r"\\?\UNC\host\share\code.exe {path}",
            "src/lib.rs {path}",
            r"src\lib.rs {path}",
            "./src/lib.rs {path}",
        ] {
            assert!(
                matches!(
                    launch(template, file, 1, 1, &gate),
                    Err(EditorError::NotFound(_))
                ),
                "{template}"
            );
        }
        assert_eq!(asked.count(), 0);
    }

    /// Declined: nothing starts, the error names the program; allowed: it starts, and the next time nobody is asked;
    /// other arguments for the same program: asked again.
    #[test]
    fn an_editor_starts_only_once_allowed() {
        let profile = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let program = marking_editor(bin.path());
        let template = template_of(&program);
        let file = bin.path().join("a.md");
        std::fs::write(&file, "x").unwrap();
        let marker = bin.path().join("started.txt");
        let asked = Arc::new(Asked::default());
        let gate = approvals(profile.path(), &asked);
        let spawned = AtomicUsize::new(0);
        let counted = |_: &Path, _: &[&OsStr]| {
            spawned.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let Err(EditorError::NotAllowed(named)) =
            launch_with(&template, &file, 1, 1, &gate, counted)
        else {
            panic!("not refused without a yes");
        };
        assert_eq!(named, program.display().to_string());
        assert_eq!(
            IpcError::from(EditorError::NotAllowed(named)).kind,
            ErrorKind::Denied
        );
        // the dialog names the full path of what would start
        assert_eq!(
            asked.asks.lock().unwrap().as_slice(),
            std::slice::from_ref(&program)
        );
        assert_eq!(spawned.load(Ordering::SeqCst), 0);

        asked.yes.store(true, Ordering::SeqCst);
        launch(&template, &file, 1, 1, &gate).unwrap();
        let started = Instant::now();
        while !marker.exists() {
            assert!(started.elapsed() < Duration::from_secs(15), "never started");
            std::thread::sleep(Duration::from_millis(50));
        }
        launch(&template, &file, 2, 1, &gate).unwrap();
        assert_eq!(asked.count(), 2);
        asked.yes.store(false, Ordering::SeqCst);
        let changed = format!("\"{}\" --evil {{path}}", program.display());
        assert!(matches!(
            launch_with(&changed, &file, 1, 1, &gate, counted),
            Err(EditorError::NotAllowed(_))
        ));
        assert_eq!(asked.count(), 3);
        assert_eq!(spawned.load(Ordering::SeqCst), 0);
    }

    /// An editor inside an open project is refused like a path never looked up: nobody asked, nothing started.
    #[test]
    fn an_editor_inside_an_open_project_is_not_found() {
        let profile = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let program = marking_editor(project.path());
        let file = project.path().join("a.md");
        std::fs::write(&file, "x").unwrap();
        let asked = Arc::new(Asked::default());
        asked.yes.store(true, Ordering::SeqCst);
        let gate = approvals(profile.path(), &asked).outside(vec![project.path().to_path_buf()]);
        let spawned = AtomicUsize::new(0);
        let result = launch_with(&template_of(&program), &file, 1, 1, &gate, |_, _| {
            spawned.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        assert!(
            matches!(result, Err(EditorError::NotFound(_))),
            "{result:?}"
        );
        assert_eq!(asked.count(), 0);
        assert_eq!(spawned.load(Ordering::SeqCst), 0);
    }

    /// What an approval is keyed by: the program found + the template's words, placeholders unfilled.
    #[test]
    fn the_program_call_holds_the_found_program_and_the_template_words() {
        let bin = tempfile::tempdir().unwrap();
        let program = marking_editor(bin.path());
        let call = program_call(&template_of(&program)).unwrap();
        assert_eq!(call.program, program);
        assert_eq!(call.args, ["--goto", "{path}:{line}"]);
        assert_eq!(call.env, []);
        assert_eq!(program_call(""), None);
        assert_eq!(program_call("definitely-not-a-program-xyz {path}"), None);
        assert_eq!(program_call("code"), None);
    }
}
