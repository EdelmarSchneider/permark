//! The commands the program runs without a window: `permark mcp` (see [`mcp`](super::mcp)) and
//! `permark context`, which prints what the person is looking at as one short paragraph and exits.
//!
//! `permark context` is made for a hook: a tool that runs it before each prompt gets a line such as
//! `Viewing docs/plan.md › Rollout (lines 40-62), selected (lines 44-46): 'ship it on Friday'.` and nothing
//! else. When there is nothing to say (the connection is off, PerMark is closed) standard output stays empty
//! and the reason goes to standard error, so a hook adds no noise to a prompt.

use std::io::Write;
use std::sync::Arc;

use super::context::Published;
use super::mcp;
use super::tools::{launch_app, Answer, Core};
use super::Subcommand;

/// Runs a command and returns the exit code.
pub(crate) fn run(command: Subcommand, identifier: &str) -> i32 {
    match command {
        Subcommand::Mcp => mcp::run_cli(identifier),
        Subcommand::Context => run_context(identifier),
    }
}

/// The paragraph for the answer of `get_context`, or why there is none.
pub(crate) fn context_line(answer: &Answer) -> Result<String, String> {
    if answer.value["status"] != "ok" {
        let reason = answer.value["message"]
            .as_str()
            .unwrap_or("PerMark has nothing to share for this folder.");
        return Err(reason.to_owned());
    }
    serde_json::from_value::<Published>(answer.value.clone())
        .map(|published| published.summary_with_comments())
        .map_err(|error| format!("PerMark answered with something unexpected: {error}"))
}

fn run_context(identifier: &str) -> i32 {
    let outcome = std::env::current_dir()
        .map_err(|error| format!("could not read the working folder: {error}"))
        .and_then(|folder| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("could not start: {error}"))?;
            let core = Core::discover(identifier, &folder, Arc::new(launch_app));
            let answer = runtime.block_on(core.get_context());
            context_line(&answer)
        });
    // The streams may not exist (a program without a console); there is nothing else to do then.
    match outcome {
        Ok(line) => {
            let _ = writeln!(std::io::stdout(), "{line}");
        }
        Err(reason) => {
            let _ = writeln!(std::io::stderr(), "permark context: {reason}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn answer(value: serde_json::Value) -> Answer {
        Answer {
            value,
            is_error: false,
        }
    }

    #[test]
    fn the_context_is_one_paragraph() {
        let line = context_line(&answer(json!({
            "status": "ok",
            "document": "docs/plan.md",
            "mode": "reading",
            "section": { "headingPath": ["Plan", "Rollout"], "startLine": 40, "endLine": 62 },
            "selection": { "text": "ship it", "headingPath": [], "startLine": 44, "endLine": 46 },
            "unsaved": false,
            "updatedAtMs": 1
        })))
        .expect("a line");
        assert_eq!(
            line,
            "Viewing docs/plan.md › Plan › Rollout (lines 40-62), selected (lines 44-46): 'ship it'."
        );
        assert!(!line.contains('\n'));
    }

    #[test]
    fn the_line_says_how_many_comments_are_open() {
        let line_with = |open: u64| {
            answer(json!({
                "status": "ok",
                "document": "docs/plan.md",
                "unsaved": false,
                "updatedAtMs": 1,
                "openComments": open
            }))
        };
        assert_eq!(
            context_line(&line_with(1)).expect("a line"),
            "Viewing docs/plan.md, 1 comment open."
        );
        assert_eq!(
            context_line(&line_with(3)).expect("a line"),
            "Viewing docs/plan.md, 3 comments open."
        );
        // None open: the line says nothing of comments, and an older window that says nothing at all is read too.
        assert_eq!(
            context_line(&line_with(0)).expect("a line"),
            "Viewing docs/plan.md."
        );
        assert_eq!(
            context_line(&answer(json!({
                "status": "ok",
                "document": "docs/plan.md",
                "unsaved": false,
                "updatedAtMs": 1
            })))
            .expect("a line"),
            "Viewing docs/plan.md."
        );
    }

    #[test]
    fn a_window_with_nothing_open_says_so() {
        let line = context_line(&answer(json!({
            "status": "ok", "document": null, "unsaved": false, "updatedAtMs": 1
        })))
        .expect("a line");
        assert_eq!(
            line,
            "PerMark is open on this project, with no document open."
        );
    }

    #[test]
    fn nothing_to_share_is_a_reason_not_a_paragraph() {
        let off = context_line(&answer(json!({ "status": "off", "message": "it is off" })));
        assert_eq!(off, Err("it is off".to_owned()));
        let closed = context_line(&answer(
            json!({ "status": "notOpen", "message": "PerMark is not open on this project." }),
        ));
        assert_eq!(
            closed,
            Err("PerMark is not open on this project.".to_owned())
        );
        let failed = context_line(&Answer {
            value: json!({ "status": "error", "message": "boom" }),
            is_error: true,
        });
        assert_eq!(failed, Err("boom".to_owned()));
    }
}
