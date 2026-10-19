//! The person's editor: a command template such as `code -g {path}:{line}:{col}` that opens a file at a line.
//!
//! Rules (directives): the program and its arguments come from the template only; a document supplies a path and
//! numbers. The template is split into arguments (quotes respected) before any placeholder is filled, so the path is
//! always inside one argument, whatever it contains. No shell. The program is an absolute path or a bare name looked
//! up on `PATH` (Windows: runnable `PATHEXT` extensions only; elsewhere also the usual editor folders), never in the
//! working folder or the project. The editor is started in the home folder, detached.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{ErrorKind, IpcError};

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

// ---- finding the program ------------------------------------------------------------------------------------------

/// Where programs are looked for.
#[derive(Debug, Clone, Default)]
pub struct SearchEnv {
    /// Absolute folders, in order.
    pub dirs: Vec<PathBuf>,
    /// Extensions a bare name may be completed with (`.exe`, ...; empty off Windows).
    pub extensions: Vec<String>,
}

const RUNNABLE: [&str; 4] = [".com", ".exe", ".bat", ".cmd"];

impl SearchEnv {
    /// From the process environment (`PATH`, `PATHEXT`, home).
    #[must_use]
    pub fn process() -> Self {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        dirs.retain(|dir| dir.is_absolute());
        let mut extensions = Vec::new();
        if cfg!(windows) {
            let pathext =
                std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
            extensions = pathext
                .split(';')
                .map(str::to_ascii_lowercase)
                .filter(|e| RUNNABLE.contains(&e.as_str()))
                .collect();
        } else {
            // a program started from the Dock | file manager gets a short PATH without these
            let home = dirs_home();
            let mut extra: Vec<PathBuf> = [
                "/usr/local/bin",
                "/opt/homebrew/bin",
                "/snap/bin",
                "/usr/bin",
            ]
            .iter()
            .map(PathBuf::from)
            .collect();
            if let Some(home) = home {
                extra.push(home.join(".local/bin"));
                extra.push(home.join("Library/Application Support/JetBrains/Toolbox/scripts"));
                extra.push(home.join(".local/share/JetBrains/Toolbox/scripts"));
            }
            for dir in extra {
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
        Self { dirs, extensions }
    }
}

fn dirs_home() -> Option<PathBuf> {
    dirs::home_dir()
}

fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// The file a program name stands for: an absolute path as it is, a bare name in the (absolute) search folders.
/// A name with a folder in it must be absolute.
#[must_use]
pub fn find_program(program: &str, env: &SearchEnv) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        return is_file(path).then(|| path.to_path_buf());
    }
    if program.contains(['/', '\\']) {
        return None;
    }
    for dir in &env.dirs {
        let plain = dir.join(program);
        if env.extensions.is_empty() {
            if is_file(&plain) {
                return Some(plain);
            }
            continue;
        }
        // a name that already ends in a runnable extension is tried as written
        let lower = program.to_ascii_lowercase();
        if env.extensions.iter().any(|e| lower.ends_with(e.as_str())) && is_file(&plain) {
            return Some(plain);
        }
        for ext in &env.extensions {
            let candidate = dir.join(format!("{program}{ext}"));
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Starts the editor on `path` (absolute, validated by the caller through the vault) at `line` | `col`.
///
/// # Errors
/// Not set, invalid template, program not found, start failure.
pub fn launch(template: &str, path: &Path, line: u32, col: u32) -> Result<(), EditorError> {
    let parsed = EditorTemplate::parse(template)
        .map_err(EditorError::Template)?
        .ok_or(EditorError::NotSet)?;
    let program = find_program(parsed.program(), &SearchEnv::process())
        .ok_or_else(|| EditorError::NotFound(parsed.program().to_owned()))?;
    let args = parsed.arguments(path, line.max(1), col.max(1))?;
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .current_dir(neutral_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console window; and never look for programs in the working folder
        command.creation_flags(0x0800_0000);
        command.env("NoDefaultCurrentDirectoryInExePath", "1");
    }
    let mut child = command
        .spawn()
        .map_err(|e| EditorError::Spawn(e.to_string()))?;
    // reaped in the background: the editor outlives this call
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The folder the editor starts in: home, else the temporary folder. Never the project.
fn neutral_dir() -> PathBuf {
    dirs_home()
        .filter(|h| h.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn program_lookup_uses_absolute_folders_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("myedit.cmd"), "x").unwrap();
        std::fs::write(dir.path().join("plain"), "x").unwrap();
        let windows = SearchEnv {
            dirs: vec![dir.path().to_path_buf()],
            extensions: vec![".exe".to_owned(), ".cmd".to_owned()],
        };
        assert_eq!(
            find_program("myedit", &windows),
            Some(dir.path().join("myedit.cmd"))
        );
        assert_eq!(
            find_program("myedit.cmd", &windows),
            Some(dir.path().join("myedit.cmd"))
        );
        assert_eq!(find_program("plain", &windows), None);
        let unix = SearchEnv {
            dirs: vec![dir.path().to_path_buf()],
            extensions: Vec::new(),
        };
        assert_eq!(find_program("plain", &unix), Some(dir.path().join("plain")));
        // names with folders must be absolute and exist; relative ones are refused
        assert_eq!(find_program("./plain", &unix), None);
        assert_eq!(find_program("sub/plain", &unix), None);
        assert_eq!(
            find_program(&dir.path().join("plain").display().to_string(), &unix),
            Some(dir.path().join("plain"))
        );
        assert_eq!(
            find_program(&dir.path().join("nope").display().to_string(), &unix),
            None
        );
        // the working folder is never searched
        assert_eq!(find_program("Cargo.toml", &SearchEnv::default()), None);
    }

    #[test]
    fn launch_reports_not_set_missing_and_bad_templates() {
        let file = Path::new("/p/a.rs");
        assert!(matches!(launch("", file, 1, 1), Err(EditorError::NotSet)));
        assert!(matches!(
            launch("definitely-not-a-program-xyz {path}", file, 1, 1),
            Err(EditorError::NotFound(_))
        ));
        assert!(matches!(
            launch("code", file, 1, 1),
            Err(EditorError::Template(TemplateProblem::NoPath))
        ));
        let error = IpcError::from(EditorError::NotSet);
        assert_eq!(error.kind, ErrorKind::EditorNotSet);
    }
}
