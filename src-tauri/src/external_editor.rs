//! The external editor command: a template such as `code -g {path}:{line}:{col}` that is run to open a file at a line.
//!
//! The template is split into arguments (quotes respected) **before** any placeholder is filled in, so the path is
//! always inside one argument, whatever it contains. The program is run directly, never through a shell. On Windows the
//! program is looked up on `PATH` with `PATHEXT`, so `code` finds `code.cmd`.
//!
//! Quoting: whitespace separates arguments; `"..."` and `'...'` keep what is inside as part of one argument (the other
//! kind of quote is plain text inside). A backslash is never an escape, so Windows paths can be written as they are.
//! Placeholders are `{path}` (absolute), `{line}` and `{col}` (both from 1); the program itself can't hold one.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// What is wrong with a template.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateProblem {
    #[error("the program is missing")]
    MissingProgram,
    #[error("a quote is not closed")]
    UnclosedQuote,
    #[error("unknown placeholder {{{0}}}")]
    UnknownPlaceholder(String),
    #[error("the program can't be a placeholder")]
    PlaceholderInProgram,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditorError {
    #[error("the editor command is not valid: {0}")]
    Template(TemplateProblem),
    #[error("can't run '{program}': not found")]
    NotFound { program: String },
    #[error("can't run '{program}': {detail}")]
    Spawn {
        program: String,
        /// The system refused the arguments (a batch file's command line can't be escaped safely).
        bad_arguments: bool,
        detail: String,
    },
}

impl EditorError {
    /// A stable code the UI words itself.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Template(_) => "template",
            Self::NotFound { .. } => "notFound",
            Self::Spawn {
                bad_arguments: true,
                ..
            } => "badArguments",
            Self::Spawn { .. } => "spawn",
        }
    }

    /// The program that could not be run, when the error is about one.
    pub fn program(&self) -> Option<&str> {
        match self {
            Self::NotFound { program } | Self::Spawn { program, .. } => Some(program),
            Self::Template(_) => None,
        }
    }

    /// Diagnostic text the UI shows as it is.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Template(problem) => Some(problem.to_string()),
            Self::Spawn { detail, .. } => Some(detail.clone()),
            Self::NotFound { .. } => None,
        }
    }
}

/// A piece of one argument.
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
    /// Parses a template. A blank one is `None`: the system's default application opens the file.
    pub fn parse(template: &str) -> Result<Option<Self>, TemplateProblem> {
        let tokens = split_args(template)?;
        let mut tokens = tokens.into_iter();
        let Some(first) = tokens.next() else {
            return Ok(None);
        };
        let program = match pieces(&first)?.as_slice() {
            [Piece::Text(text)] if !text.is_empty() => text.clone(),
            [] | [Piece::Text(_)] => return Err(TemplateProblem::MissingProgram),
            _ => return Err(TemplateProblem::PlaceholderInProgram),
        };
        let args = tokens
            .map(|token| pieces(&token))
            .collect::<Result<_, _>>()?;
        Ok(Some(Self { program, args }))
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    /// The arguments with the placeholders filled in. The path is never split or re-read for placeholders.
    pub fn arguments(&self, path: &Path, line: u32, col: u32) -> Vec<OsString> {
        let (line, col) = (line.max(1), col.max(1));
        self.args
            .iter()
            .map(|arg| {
                let mut out = OsString::new();
                for piece in arg {
                    match piece {
                        Piece::Text(text) => out.push(text),
                        Piece::Path => out.push(path),
                        Piece::Line => out.push(line.to_string()),
                        Piece::Col => out.push(col.to_string()),
                    }
                }
                out
            })
            .collect()
    }
}

/// Splits a template into arguments. Quotes group (and are dropped); an empty pair of quotes is an empty argument.
fn split_args(template: &str) -> Result<Vec<String>, TemplateProblem> {
    let mut args = Vec::new();
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
                    args.push(std::mem::take(&mut current));
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
        return Err(TemplateProblem::UnclosedQuote);
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

/// Cuts one argument into text and placeholders. Only `{name}` with letters, digits or `_` is a placeholder; any other
/// brace is text.
fn pieces(arg: &str) -> Result<Vec<Piece>, TemplateProblem> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = arg;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if name_len > 0 && after[name_len..].starts_with('}') {
            let piece = match &after[..name_len] {
                "path" => Piece::Path,
                "line" => Piece::Line,
                "col" => Piece::Col,
                other => return Err(TemplateProblem::UnknownPlaceholder(other.to_owned())),
            };
            text.push_str(&rest[..open]);
            if !text.is_empty() {
                out.push(Piece::Text(std::mem::take(&mut text)));
            }
            out.push(piece);
            rest = &after[name_len + 1..];
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

// ---- finding the program ----------------------------------------------------------------------------------

/// Where programs are looked for, and the extensions a bare name may be completed with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchEnv {
    /// The folders of `PATH`, in order.
    pub dirs: Vec<PathBuf>,
    /// Windows style lookup: the name is completed with these extensions (lowercase, with the dot), in order. Empty:
    /// the name is used as it is and must have an execute permission.
    pub extensions: Vec<String>,
}

/// The extensions of `PATHEXT` that a process can be started from directly (or through the batch interpreter). The
/// others (`.vbs`, `.js`...) are left out: they are not programs for `CreateProcess`.
const RUNNABLE_EXTENSIONS: [&str; 4] = [".com", ".exe", ".bat", ".cmd"];

impl SearchEnv {
    /// The search of this machine: `PATH`, and on Windows `PATHEXT`.
    pub fn system() -> Self {
        let dirs = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        let extensions = if cfg!(windows) {
            let pathext =
                std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
            runnable_extensions(&pathext)
        } else {
            Vec::new()
        };
        Self { dirs, extensions }
    }
}

/// The runnable extensions of a `PATHEXT` value, lowercase, in its order.
fn runnable_extensions(pathext: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ext in pathext.split(';').map(|e| e.trim().to_ascii_lowercase()) {
        if RUNNABLE_EXTENSIONS.contains(&ext.as_str()) && !out.contains(&ext) {
            out.push(ext);
        }
    }
    out
}

/// Finds the file a program name stands for. A name with a folder in it must be absolute; a bare name is looked for in
/// the (absolute) folders of `PATH`, so nothing in the working folder or in the project is ever picked up.
pub fn find_program(program: &str, env: &SearchEnv) -> Option<PathBuf> {
    if program.contains(['/', '\\']) {
        let path = Path::new(program);
        return path
            .is_absolute()
            .then(|| first_runnable(path.parent()?, path.file_name()?, env))
            .flatten();
    }
    env.dirs
        .iter()
        .filter(|dir| dir.is_absolute())
        .find_map(|dir| first_runnable(dir, OsStr::new(program), env))
}

/// `dir/name` completed the way `env` says, the first that exists and can run.
fn first_runnable(dir: &Path, name: &OsStr, env: &SearchEnv) -> Option<PathBuf> {
    let name = name.to_string_lossy();
    let lower = name.to_ascii_lowercase();
    let mut candidates: Vec<String> = Vec::new();
    if env.extensions.is_empty() {
        candidates.push(name.to_string());
    } else {
        if env.extensions.iter().any(|ext| lower.ends_with(ext)) {
            candidates.push(name.to_string());
        }
        candidates.extend(env.extensions.iter().map(|ext| format!("{name}{ext}")));
    }
    candidates
        .into_iter()
        .map(|candidate| dir.join(candidate))
        .find(|path| path.is_file() && (!env.extensions.is_empty() || is_executable(path)))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

// ---- running ------------------------------------------------------------------------------------------------

/// Starts the editor for `path` and returns the child. The program is found with `env`; nothing goes through a shell.
pub fn launch(
    template: &EditorTemplate,
    path: &Path,
    line: u32,
    col: u32,
    env: &SearchEnv,
) -> Result<Child, EditorError> {
    let program = template.program();
    let Some(found) = find_program(program, env) else {
        return Err(EditorError::NotFound {
            program: program.to_owned(),
        });
    };
    let mut command = Command::new(found);
    command
        .args(template.arguments(path, line, col))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command);
    command
        .spawn()
        .map_err(|source| spawn_error(program, &source))
}

fn spawn_error(program: &str, source: &io::Error) -> EditorError {
    if source.kind() == io::ErrorKind::NotFound {
        return EditorError::NotFound {
            program: program.to_owned(),
        };
    }
    EditorError::Spawn {
        program: program.to_owned(),
        bad_arguments: source.kind() == io::ErrorKind::InvalidInput,
        detail: source.to_string(),
    }
}

/// A batch file such as `code.cmd` would flash a console window.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Runs the editor and lets it go: a thread collects the exit status so no zombie is left behind.
pub fn run(template: &EditorTemplate, path: &Path, line: u32, col: u32) -> Result<(), EditorError> {
    let mut child = launch(template, path, line, col, &SearchEnv::system())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(template: &str) -> EditorTemplate {
        EditorTemplate::parse(template)
            .expect("valid")
            .expect("not blank")
    }

    fn args(template: &str, path: &str, line: u32, col: u32) -> Vec<String> {
        parse(template)
            .arguments(Path::new(path), line, col)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    // ---- splitting ----

    #[test]
    fn a_blank_template_means_the_system_default_app() {
        for blank in ["", "   ", "\t\n "] {
            assert_eq!(EditorTemplate::parse(blank), Ok(None), "{blank:?}");
        }
    }

    #[test]
    fn whitespace_separates_arguments() {
        assert_eq!(
            split_args("code  -g\t{path}:{line}"),
            Ok(vec!["code".into(), "-g".into(), "{path}:{line}".into()])
        );
    }

    #[test]
    fn quotes_group_and_are_dropped() {
        assert_eq!(
            split_args(r#""C:\Program Files\Code\code.exe" -g "{path}""#),
            Ok(vec![
                r"C:\Program Files\Code\code.exe".into(),
                "-g".into(),
                "{path}".into()
            ])
        );
        assert_eq!(
            split_args("zed 'a b' --flag=\"c d\"e"),
            Ok(vec!["zed".into(), "a b".into(), "--flag=c de".into()])
        );
    }

    #[test]
    fn the_other_kind_of_quote_is_text_inside_a_quote() {
        assert_eq!(
            split_args(r#"ed "it's" 'say "hi"'"#),
            Ok(vec!["ed".into(), "it's".into(), r#"say "hi""#.into()])
        );
    }

    #[test]
    fn an_empty_pair_of_quotes_is_an_empty_argument() {
        assert_eq!(
            split_args(r#"ed "" '' x"#),
            Ok(vec!["ed".into(), String::new(), String::new(), "x".into()])
        );
    }

    #[test]
    fn a_backslash_is_never_an_escape() {
        assert_eq!(
            split_args(r"C:\tools\ed.exe C:\a\b"),
            Ok(vec![r"C:\tools\ed.exe".into(), r"C:\a\b".into()])
        );
        // the quote after a backslash still closes the quote
        assert_eq!(
            split_args(r#"ed "a\" b"#),
            Ok(vec!["ed".into(), r"a\".into(), "b".into()])
        );
    }

    #[test]
    fn an_unclosed_quote_is_an_error() {
        for bad in [r#"code "-g {path}"#, "code 'x", r#"code "a' b"#] {
            assert_eq!(
                split_args(bad),
                Err(TemplateProblem::UnclosedQuote),
                "{bad}"
            );
        }
    }

    // ---- placeholders ----

    #[test]
    fn placeholders_are_filled_after_the_split_so_a_path_stays_one_argument() {
        for path in [
            "/home/me/my project/a b.rs",
            r"C:\Users\Jo Smith\proj\a b.rs",
            "/tmp/it's \"quoted\" {line}.rs",
            "/tmp/-rf --no-sandbox.rs",
        ] {
            assert_eq!(
                args("code -g {path}:{line}:{col}", path, 42, 7),
                ["-g".to_owned(), format!("{path}:42:7")],
                "{path}"
            );
            assert_eq!(
                args("idea --line {line} {path}", path, 3, 1),
                ["--line".to_owned(), "3".to_owned(), path.to_owned()],
                "{path}"
            );
        }
    }

    #[test]
    fn what_is_substituted_is_not_scanned_again() {
        assert_eq!(
            args("ed {path}", "/x/{line}/{col}.rs", 9, 9),
            ["/x/{line}/{col}.rs"]
        );
    }

    #[test]
    fn line_and_column_start_at_one() {
        assert_eq!(args("ed {line}:{col}", "/a", 0, 0), ["1:1"]);
        assert_eq!(args("ed {line}:{col}", "/a", 12, 0), ["12:1"]);
    }

    #[test]
    fn a_placeholder_can_be_repeated_and_sit_inside_text() {
        assert_eq!(
            args("ed +{line} --at={path}@{col} {path}", "/a.rs", 5, 2),
            ["+5", "--at=/a.rs@2", "/a.rs"]
        );
    }

    #[test]
    fn braces_that_are_not_placeholders_are_text() {
        assert_eq!(
            args("ed {} { path } {a-b} {{path}} { x", "/a", 1, 1),
            ["{}", "{", "path", "}", "{a-b}", "{/a}", "{", "x"]
        );
    }

    #[test]
    fn an_unknown_placeholder_is_an_error() {
        assert_eq!(
            EditorTemplate::parse("code {file}"),
            Err(TemplateProblem::UnknownPlaceholder("file".into()))
        );
        assert_eq!(
            EditorTemplate::parse("code {PATH}"),
            Err(TemplateProblem::UnknownPlaceholder("PATH".into()))
        );
    }

    #[test]
    fn the_program_can_not_be_a_placeholder_or_empty() {
        for bad in ["{path} -x", "ed{path} x", "{line}"] {
            assert_eq!(
                EditorTemplate::parse(bad),
                Err(TemplateProblem::PlaceholderInProgram),
                "{bad}"
            );
        }
        for bad in [r#""" {path}"#, "'' x"] {
            assert_eq!(
                EditorTemplate::parse(bad),
                Err(TemplateProblem::MissingProgram),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_presets_of_the_settings_all_parse() {
        for template in [
            "code -g {path}:{line}:{col}",
            "cursor -g {path}:{line}:{col}",
            "zed {path}:{line}:{col}",
            "subl {path}:{line}:{col}",
            "idea --line {line} {path}",
        ] {
            let parsed = parse(template);
            assert_eq!(parsed.program(), template.split(' ').next().unwrap_or(""));
        }
    }

    // ---- finding the program ----

    fn env_in(dir: &Path, extensions: &[&str]) -> SearchEnv {
        SearchEnv {
            dirs: vec![dir.to_path_buf()],
            extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        }
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "x").expect("write");
        path
    }

    #[test]
    fn only_the_runnable_extensions_of_pathext_are_kept_in_order() {
        assert_eq!(
            runnable_extensions(".COM;.EXE;.BAT;.CMD;.VBS;.JS;.MSC"),
            [".com", ".exe", ".bat", ".cmd"]
        );
        assert_eq!(runnable_extensions(".cmd; .EXE;.cmd;"), [".cmd", ".exe"]);
        assert_eq!(runnable_extensions(".VBS;.JS"), Vec::<String>::new());
    }

    #[test]
    fn a_bare_name_is_completed_with_the_extensions_like_the_windows_shell_does() {
        let d = tempfile::tempdir().expect("tempdir");
        let cmd = touch(d.path(), "code.cmd");
        let exe = touch(d.path(), "tool.exe");
        touch(d.path(), "script.vbs");
        touch(d.path(), "plain");
        let env = env_in(d.path(), &[".com", ".exe", ".bat", ".cmd"]);
        assert_eq!(find_program("code", &env), Some(cmd.clone()));
        assert_eq!(find_program("tool", &env), Some(exe));
        // a name that carries its extension is used as it is
        assert_eq!(find_program("code.cmd", &env), Some(cmd));
        assert_eq!(find_program("script", &env), None);
        assert_eq!(
            find_program("plain", &env),
            None,
            "no extension, no program"
        );
        assert_eq!(find_program("missing", &env), None);
    }

    #[test]
    fn the_extensions_are_tried_in_pathext_order() {
        let d = tempfile::tempdir().expect("tempdir");
        touch(d.path(), "ed.bat");
        let exe = touch(d.path(), "ed.exe");
        touch(d.path(), "ed.cmd");
        let env = env_in(d.path(), &[".com", ".exe", ".bat", ".cmd"]);
        assert_eq!(find_program("ed", &env), Some(exe));
    }

    #[test]
    fn the_first_folder_of_path_wins() {
        let a = tempfile::tempdir().expect("tempdir");
        let b = tempfile::tempdir().expect("tempdir");
        let in_b = touch(b.path(), "ed.exe");
        let env = SearchEnv {
            dirs: vec![a.path().to_path_buf(), b.path().to_path_buf()],
            extensions: vec![".exe".into()],
        };
        assert_eq!(find_program("ed", &env), Some(in_b));
        let in_a = touch(a.path(), "ed.exe");
        assert_eq!(find_program("ed", &env), Some(in_a));
    }

    #[test]
    fn relative_folders_of_path_are_ignored() {
        let d = tempfile::tempdir().expect("tempdir");
        touch(d.path(), "ed.exe");
        let env = SearchEnv {
            dirs: vec![PathBuf::new(), PathBuf::from("."), PathBuf::from("bin")],
            extensions: vec![".exe".into()],
        };
        assert_eq!(find_program("ed", &env), None);
    }

    #[test]
    fn a_name_with_a_folder_must_be_absolute() {
        let d = tempfile::tempdir().expect("tempdir");
        let exe = touch(d.path(), "ed.exe");
        let env = env_in(d.path(), &[".exe"]);
        let absolute = exe.display().to_string();
        assert_eq!(find_program(&absolute, &env), Some(exe.clone()));
        // the extension may be left out, as in a shell
        let bare = d.path().join("ed").display().to_string();
        assert_eq!(find_program(&bare, &env), Some(exe));
        for relative in ["./ed.exe", "bin/ed.exe", r".\ed.exe", "../ed"] {
            assert_eq!(find_program(relative, &env), None, "{relative}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn without_extensions_the_file_must_be_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().expect("tempdir");
        let path = touch(d.path(), "ed");
        let env = env_in(d.path(), &[]);
        assert_eq!(find_program("ed", &env), None);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert_eq!(find_program("ed", &env), Some(path));
    }

    #[test]
    fn a_folder_is_not_a_program() {
        let d = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(d.path().join("ed.exe")).expect("mkdir");
        assert_eq!(find_program("ed", &env_in(d.path(), &[".exe"])), None);
    }

    // ---- running ----

    #[test]
    fn a_program_that_is_not_there_is_reported_with_its_name() {
        let d = tempfile::tempdir().expect("tempdir");
        let template = parse("no-such-editor {path}");
        let error = launch(&template, Path::new("/a.rs"), 1, 1, &env_in(d.path(), &[]))
            .expect_err("not found");
        assert_eq!(
            error,
            EditorError::NotFound {
                program: "no-such-editor".into()
            }
        );
        assert_eq!(error.reason(), "notFound");
        assert_eq!(error.program(), Some("no-such-editor"));
        assert_eq!(error.to_string(), "can't run 'no-such-editor': not found");
    }

    /// Tests that start a script take turns: on Linux, running a file that another thread has open for writing at that
    /// moment (inherited by a forked child) fails with "text file busy".
    fn one_spawn_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        TURN.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A script that writes its first two arguments, one per line, to `args.txt` beside it.
    #[cfg(windows)]
    fn recorder(dir: &Path) -> PathBuf {
        let path = dir.join("rec.cmd");
        std::fs::write(
            &path,
            "@echo off\r\n(echo %~1)>\"%~dp0args.txt\"\r\n(echo %~2)>>\"%~dp0args.txt\"\r\n",
        )
        .expect("script");
        path
    }

    #[cfg(unix)]
    fn recorder(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("rec");
        std::fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\n' \"$1\" > \"$(dirname \"$0\")/args.txt\"\nprintf '%s\\n' \"$2\" >> \"$(dirname \"$0\")/args.txt\"\n",
        )
        .expect("script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// The lookup the recorder needs: on Windows `rec` completes to `rec.cmd`.
    fn recorder_env(dir: &Path) -> SearchEnv {
        SearchEnv {
            dirs: vec![dir.to_path_buf()],
            extensions: if cfg!(windows) {
                vec![".cmd".into()]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn the_editor_gets_the_path_as_one_argument_and_runs_without_a_shell() {
        let _one_at_a_time = one_spawn_at_a_time();
        let d = tempfile::tempdir().expect("tempdir");
        let script = recorder(d.path());
        // a path with spaces, as a real project folder may have
        let target = d.path().join("my docs").join("a b.rs");
        let template = parse(&format!(
            "\"{}\" -g {{path}}:{{line}}:{{col}}",
            script.display()
        ));
        let mut child = launch(&template, &target, 42, 7, &recorder_env(d.path())).expect("launch");
        assert!(child.wait().expect("wait").success());
        let written = std::fs::read_to_string(d.path().join("args.txt")).expect("args.txt");
        let lines: Vec<&str> = written.lines().map(str::trim_end).collect();
        assert_eq!(lines, ["-g", &format!("{}:42:7", target.display())]);
    }

    #[test]
    fn a_program_found_on_path_with_its_extension_completed_is_the_one_run() {
        let _one_at_a_time = one_spawn_at_a_time();
        let d = tempfile::tempdir().expect("tempdir");
        let script = recorder(d.path());
        let stem = script
            .file_stem()
            .expect("stem")
            .to_string_lossy()
            .into_owned();
        let template = parse(&format!("{stem} one {{line}}"));
        let mut child = launch(
            &template,
            Path::new("/ignored"),
            9,
            1,
            &recorder_env(d.path()),
        )
        .expect("launch");
        assert!(child.wait().expect("wait").success());
        let written = std::fs::read_to_string(d.path().join("args.txt")).expect("args.txt");
        assert_eq!(
            written.lines().map(str::trim_end).collect::<Vec<_>>(),
            ["one", "9"]
        );
    }

    /// Rust refuses to put a line break into the command line of a batch file; that surfaces as an error to show.
    #[cfg(windows)]
    #[test]
    fn arguments_a_batch_file_can_not_take_safely_are_an_error() {
        let d = tempfile::tempdir().expect("tempdir");
        let script = recorder(d.path());
        let template = parse(&format!("\"{}\" {{path}}", script.display()));
        let env = env_in(d.path(), &[".cmd"]);
        let error = launch(&template, Path::new("C:\\a\nb.rs"), 1, 1, &env).expect_err("refused");
        assert_eq!(error.reason(), "badArguments");
        assert!(error.detail().is_some_and(|d| !d.is_empty()));
    }

    #[test]
    fn errors_carry_what_the_ui_needs_to_word_them() {
        let template = EditorError::Template(TemplateProblem::UnknownPlaceholder("x".into()));
        assert_eq!(template.reason(), "template");
        assert_eq!(template.program(), None);
        assert_eq!(
            template.detail().as_deref(),
            Some("unknown placeholder {x}")
        );
        let spawn = EditorError::Spawn {
            program: "ed".into(),
            bad_arguments: false,
            detail: "boom".into(),
        };
        assert_eq!(spawn.reason(), "spawn");
        assert_eq!(spawn.program(), Some("ed"));
        assert_eq!(spawn.detail().as_deref(), Some("boom"));
    }
}
