//! The error type returned by every command. It serializes as `{ kind, message, ... }`: the UI branches on
//! `kind` and words the error itself in the person's language (`subject`, `detail` and `reason` fill its
//! blanks). `message` is English, for logs and for a `kind` a UI does not know.

use std::io;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::edit::EditError;
use crate::external_editor::EditorError;
use crate::fileops::FileOpError;
use crate::git::GitError;
use crate::meta::MetaError;
use crate::render::RenderError;
use crate::session::SessionError;
use crate::shell_integration::ShellError;
use crate::vault::VaultError;
use crate::watch::WatchError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Watch(#[from] WatchError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Meta(#[from] MetaError),
    #[error(transparent)]
    Edit(#[from] EditError),
    #[error(transparent)]
    FileOps(#[from] FileOpError),
    #[error(transparent)]
    Shell(#[from] ShellError),
    #[error(transparent)]
    Editor(#[from] EditorError),
    #[error("no folder is open")]
    NoVault,
    /// A newer request to open a folder arrived first, so this one was dropped.
    #[error("another folder was opened in the meantime")]
    Superseded,
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    /// A short stable identifier for the UI.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Vault(e) => e.kind(),
            Self::Render(RenderError::Io { source, .. }) => {
                if source.kind() == std::io::ErrorKind::NotFound {
                    "notFound"
                } else {
                    "io"
                }
            }
            Self::Watch(_) => "watch",
            Self::Git(GitError::Git(_)) => "git",
            Self::Git(GitError::WorkerGone) => "gitUnavailable",
            Self::Session(_) => "session",
            Self::Meta(e) => e.kind(),
            Self::Edit(e) => e.kind(),
            Self::FileOps(e) => e.kind(),
            Self::Shell(_) => "shell",
            Self::Editor(_) => "editor",
            Self::NoVault => "noVault",
            Self::Superseded => "superseded",
            Self::Internal(_) => "internal",
        }
    }
}

/// What a message needs besides its kind. Every field is optional; the UI's wording for a kind says which it uses.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Details {
    /// What the error is about: a path or a file name.
    pub subject: Option<String>,
    /// More from the system or a library (an OS message, a parser's complaint). Not translated: it is
    /// diagnostic text, shown after the UI's own sentence.
    pub detail: Option<String>,
    /// A stable code that narrows the kind (why a name cannot be used).
    pub reason: Option<String>,
}

impl Details {
    fn subject(subject: &str) -> Self {
        Self {
            subject: Some(subject.to_owned()),
            ..Self::default()
        }
    }

    fn detail(detail: &str) -> Self {
        Self {
            detail: Some(detail.to_owned()),
            ..Self::default()
        }
    }

    fn io(path: &str, source: &io::Error) -> Self {
        Self {
            subject: Some(path.to_owned()),
            detail: Some(source.to_string()),
            reason: None,
        }
    }
}

fn vault_details(e: &VaultError) -> Details {
    match e {
        VaultError::Escapes(path) | VaultError::Invalid(path) | VaultError::NotFound(path) => {
            Details::subject(path)
        }
        VaultError::Io { path, source } => Details::io(path, source),
    }
}

fn file_op_details(e: &FileOpError) -> Details {
    match e {
        FileOpError::Vault(e) => vault_details(e),
        FileOpError::Exists(name)
        | FileOpError::IntoItself(name)
        | FileOpError::OtherRoot(name)
        | FileOpError::Protected(name)
        | FileOpError::Overlap(name) => Details::subject(name),
        FileOpError::InvalidName(problem) => Details {
            reason: Some(problem.code().to_owned()),
            ..Details::default()
        },
        FileOpError::HoldsWorktree { path, worktree } => Details {
            subject: Some(path.clone()),
            detail: Some(worktree.clone()),
            reason: None,
        },
        FileOpError::SimpleMode
        | FileOpError::Nothing
        | FileOpError::UnknownPlan
        | FileOpError::UnknownOperation => Details::default(),
        FileOpError::Io { path, source } => Details::io(path, source),
        FileOpError::PartlyMoved { reason, left } => Details {
            subject: Some(
                left.iter()
                    .map(|m| m.to.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            detail: file_op_details(reason).detail,
            reason: None,
        },
        FileOpError::Trash(message) => Details::detail(message),
    }
}

impl AppError {
    /// The blanks the UI fills in when it words this error.
    pub fn details(&self) -> Details {
        match self {
            Self::Vault(e) => vault_details(e),
            Self::Render(RenderError::Io { path, source })
            | Self::Session(SessionError::Io { path, source }) => Details::io(path, source),
            Self::Watch(WatchError::Notify(message) | WatchError::Spawn(message))
            | Self::Git(GitError::Git(message))
            | Self::Session(SessionError::Json(message))
            | Self::Internal(message) => Details::detail(message),
            Self::Meta(e) => match e {
                MetaError::Unreadable(problem) => Details {
                    // the line of the first mistake, when it is known
                    subject: problem.line.map(|line| line.to_string()),
                    detail: Some(problem.message.clone()),
                    reason: None,
                },
                MetaError::Worktree(path) => Details::subject(path),
                MetaError::Io { path, source } => Details::io(path, source),
                MetaError::Vault(e) => vault_details(e),
            },
            Self::Edit(e) => match e {
                EditError::Vault(e) => vault_details(e),
                EditError::Binary(name)
                | EditError::TooLarge(name)
                | EditError::NotMarkdown(name)
                | EditError::Encoding(name)
                | EditError::Exists(name) => Details::subject(name),
                EditError::InvalidName(problem) => Details {
                    reason: Some(problem.code().to_owned()),
                    ..Details::default()
                },
                EditError::Unsupported(_) => Details::default(),
                EditError::InvalidData(message) => Details::detail(message),
                EditError::Io { path, source } => Details::io(path, source),
            },
            Self::FileOps(e) => file_op_details(e),
            Self::Shell(ShellError::Registry(source)) => Details::detail(&source.to_string()),
            Self::Shell(ShellError::Unsupported) => Details::default(),
            Self::Editor(e) => Details {
                subject: e.program().map(str::to_owned),
                detail: e.detail(),
                reason: Some(e.reason().to_owned()),
            },
            Self::Git(GitError::WorkerGone) | Self::NoVault | Self::Superseded => {
                Details::default()
            }
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let details = self.details();
        let mut s = serializer.serialize_struct("AppError", 5)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        for (name, value) in [
            ("subject", &details.subject),
            ("detail", &details.detail),
            ("reason", &details.reason),
        ] {
            match value {
                Some(value) => s.serialize_field(name, value)?,
                None => s.skip_field(name)?,
            }
        }
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn json(e: &AppError) -> serde_json::Value {
        serde_json::to_value(e).expect("json")
    }

    #[test]
    fn serializes_as_kind_and_message_with_the_blanks_the_ui_fills_in() {
        let e = AppError::from(VaultError::Escapes("../x".to_owned()));
        assert_eq!(
            json(&e),
            serde_json::json!({
                "kind": "pathEscape",
                "message": "path is outside the folder: ../x",
                "subject": "../x"
            })
        );
        assert_eq!(
            json(&AppError::NoVault),
            serde_json::json!({"kind": "noVault", "message": "no folder is open"})
        );
    }

    #[test]
    fn a_name_problem_travels_as_a_code_not_as_english() {
        let e = AppError::from(crate::edit::EditError::InvalidName(
            crate::edit::NameProblem::Reserved,
        ));
        assert_eq!(
            json(&e),
            serde_json::json!({
                "kind": "invalidName",
                "message": "That name is reserved by the system",
                "reason": "reserved"
            })
        );
        let e = AppError::from(crate::fileops::FileOpError::InvalidName(
            crate::edit::NameProblem::ForbiddenChars,
        ));
        assert_eq!(json(&e)["reason"], "forbiddenChars");
    }

    #[test]
    fn io_errors_carry_the_path_and_the_system_text_separately() {
        let e = AppError::from(crate::edit::EditError::Io {
            path: "docs/a.md".to_owned(),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        });
        let value = json(&e);
        assert_eq!(value["kind"], "denied");
        assert_eq!(value["subject"], "docs/a.md");
        assert!(value["detail"].as_str().is_some_and(|d| !d.is_empty()));
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn a_move_that_stopped_halfway_names_what_stayed_and_why_the_system_said() {
        let e = AppError::from(crate::fileops::FileOpError::PartlyMoved {
            reason: Box::new(crate::fileops::FileOpError::Io {
                path: "a.md".to_owned(),
                source: io::Error::other("disk full"),
            }),
            left: vec![
                crate::fileops::Move {
                    from: "a.md".to_owned(),
                    to: "x/a.md".to_owned(),
                },
                crate::fileops::Move {
                    from: "b.md".to_owned(),
                    to: "x/b.md".to_owned(),
                },
            ],
        });
        let value = json(&e);
        assert_eq!(value["kind"], "partlyMoved");
        assert_eq!(value["subject"], "x/a.md, x/b.md");
        assert_eq!(value["detail"], "disk full");
    }

    #[test]
    fn a_file_manager_error_carries_the_system_text_as_detail() {
        let e = AppError::from(ShellError::Registry(io::Error::from(
            io::ErrorKind::PermissionDenied,
        )));
        let value = json(&e);
        assert_eq!(value["kind"], "shell");
        assert!(value["detail"].as_str().is_some_and(|d| !d.is_empty()));
        let unsupported = json(&AppError::from(ShellError::Unsupported));
        assert_eq!(unsupported["kind"], "shell");
        assert!(unsupported.get("detail").is_none());
    }

    #[test]
    fn an_editor_error_names_the_program_and_why() {
        let e = AppError::from(EditorError::NotFound {
            program: "code".to_owned(),
        });
        assert_eq!(
            json(&e),
            serde_json::json!({
                "kind": "editor",
                "message": "can't run 'code': not found",
                "subject": "code",
                "reason": "notFound"
            })
        );
        let e = AppError::from(EditorError::Template(
            crate::external_editor::TemplateProblem::UnclosedQuote,
        ));
        let value = json(&e);
        assert_eq!(value["reason"], "template");
        assert_eq!(value["detail"], "a quote is not closed");
        assert!(value.get("subject").is_none());
    }

    #[test]
    fn kinds_without_blanks_serialize_without_extra_keys() {
        for e in [
            AppError::NoVault,
            AppError::Superseded,
            AppError::from(GitError::WorkerGone),
            AppError::from(crate::fileops::FileOpError::SimpleMode),
        ] {
            let value = json(&e);
            let keys: Vec<&String> = value.as_object().expect("object").keys().collect();
            assert_eq!(keys, ["kind", "message"], "{e}");
        }
    }

    #[test]
    fn kinds_distinguish_not_found_from_other_io_errors() {
        let nf = AppError::Render(RenderError::Io {
            path: "a.md".to_owned(),
            source: io::Error::from(io::ErrorKind::NotFound),
        });
        assert_eq!(nf.kind(), "notFound");
        let denied = AppError::Render(RenderError::Io {
            path: "a.md".to_owned(),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        });
        assert_eq!(denied.kind(), "io");
        assert_eq!(AppError::from(GitError::Git("x".to_owned())).kind(), "git");
        assert_eq!(
            AppError::from(GitError::WorkerGone).kind(),
            "gitUnavailable"
        );
        assert_eq!(AppError::Internal("x".to_owned()).kind(), "internal");
        assert_eq!(AppError::Superseded.kind(), "superseded");
        assert_eq!(
            AppError::from(VaultError::NotFound("p".to_owned())).kind(),
            "notFound"
        );
    }
}
