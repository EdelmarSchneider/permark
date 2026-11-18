//! Permission questions: what the agent told of a tool call so far, and which questions are plainly work on the
//! copy's files (allowed once by rule) rather than a card for the person. Fails closed: any doubt -> a card.
//!
//! The kind, locations and input of a tool call are the agent's own description, not a grant: they are read from
//! everything the agent told of the call (its `tool_call`, every `tool_call_update` since, the question's own fields
//! last), and the rule only allows what that description keeps inside the copy.

use std::path::{Component, Path, PathBuf};

use agent_client_protocol_schema::v1::{
    PermissionOptionId, PermissionOptionKind, RequestPermissionRequest, ToolCall, ToolCallStatus,
    ToolCallUpdateFields, ToolKind,
};
use permark_core::paths::{Vault, VaultPath};
use serde_json::Value;

use super::{tool_kind, tool_status};
use crate::dto::{AcpToolKind, AcpToolStatus, AcpUpdate};
use crate::work::kept_out;

/// Keys of a tool's input that name files (as Claude, Codex, Gemini and opencode spell them); a relative one is taken
/// from the agent's folder, the copy.
const PATH_KEYS: [&str; 15] = [
    "file_path",
    "filePath",
    "path",
    "notebook_path",
    "abs_path",
    "source",
    "old_path",
    "oldPath",
    "from",
    "destination",
    "dest",
    "new_path",
    "newPath",
    "target",
    "to",
];

/// What the agent told of one tool call so far.
#[derive(Debug, Default, Clone)]
pub(super) struct ToolLine {
    pub title: String,
    /// `None`: the agent never said.
    pub kind: Option<ToolKind>,
    pub status: ToolCallStatus,
    pub locations: Vec<PathBuf>,
    /// The tool's own arguments.
    pub raw_input: Option<Value>,
}

impl ToolLine {
    /// A tool call as the agent announced it.
    pub fn of(call: ToolCall) -> Self {
        Self {
            title: call.title,
            kind: Some(call.kind),
            status: call.status,
            locations: call.locations.into_iter().map(|l| l.path).collect(),
            raw_input: call.raw_input,
        }
    }

    /// The fields an update sets, over what was told before.
    pub fn apply(&mut self, fields: &ToolCallUpdateFields) {
        if let Some(title) = &fields.title {
            self.title.clone_from(title);
        }
        if let Some(kind) = fields.kind {
            self.kind = Some(kind);
        }
        if let Some(status) = fields.status {
            self.status = status;
        }
        if let Some(locations) = &fields.locations {
            self.locations = locations.iter().map(|l| l.path.clone()).collect();
        }
        if let Some(raw) = &fields.raw_input {
            self.raw_input = Some(raw.clone());
        }
    }

    pub fn kind(&self) -> AcpToolKind {
        self.kind.map_or(AcpToolKind::Other, tool_kind)
    }

    /// What the window shows of it (an update that changes none of it is not told).
    pub fn shown(&self) -> (&str, AcpToolKind, AcpToolStatus) {
        (&self.title, self.kind(), tool_status(self.status))
    }

    pub fn update(&self, id: &str) -> AcpUpdate {
        let (title, kind, status) = self.shown();
        AcpUpdate::Tool {
            id: id.to_owned(),
            title: title.to_owned(),
            kind,
            status,
        }
    }

    /// The files its input names, absolute (a relative one from the copy's folder).
    fn input_paths(&self, copy: &Vault) -> Vec<PathBuf> {
        let Some(Value::Object(input)) = &self.raw_input else {
            return Vec::new();
        };
        PATH_KEYS
            .iter()
            .filter_map(|key| input.get(*key)?.as_str())
            .filter(|named| !named.is_empty())
            .map(|named| {
                let path = Path::new(named);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    copy.main().path().join(path)
                }
            })
            .collect()
    }
}

/// Whether `path` (as the agent wrote it) is the copy's folder or inside it, also on its real location (a link the
/// agent made in the copy does not count as inside).
pub(super) fn inside(copy: &Vault, path: &Path) -> bool {
    path.is_absolute()
        && !path.components().any(|c| matches!(c, Component::ParentDir))
        && copy
            .relativize(path)
            .is_some_and(|relative| copy.resolve(&relative).is_ok())
}

/// Whether `path` is agent settings | project machinery inside the copy (what a next agent start would load), or
/// may be. Windows opens one file under several spellings (a trailing dot | space, a `::$DATA` stream, an 8.3 short
/// name): a spelling the path grammar refuses counts as kept out, and so does the name the file system resolves it to.
fn kept_out_in(copy: &Vault, path: &Path) -> bool {
    let Some(relative) = copy.relativize(path) else {
        return false;
    };
    if relative.is_root() {
        return false;
    }
    let Ok(parsed) = VaultPath::parse(relative.as_str()) else {
        return true;
    };
    let Some(real) = copy
        .resolve(&parsed)
        .ok()
        .and_then(|abs| copy.relativize(&abs))
    else {
        return true;
    };
    [relative.as_str(), parsed.as_str(), real.as_str()]
        .iter()
        .any(|p| kept_out(p))
}

/// Whether the tool call is plainly work on the copy's files, so the question can be allowed once by rule: nothing
/// it names is agent settings | project machinery (whatever its kind); and it reads | searches | edits | deletes |
/// moves, names at least one location, and every location and every file its input names (a move's destination
/// among them) is inside the copy. Anything else (commands, fetches, mode switches, thinking (a subagent | task tool
/// starts tools of its own), an unknown | missing kind, no locations) is the person's to answer.
pub(super) fn plain_work(tool: &ToolLine, copy: &Vault) -> bool {
    let named = tool.input_paths(copy);
    let all = || tool.locations.iter().chain(named.iter());
    if all().any(|path| kept_out_in(copy, path)) {
        return false;
    }
    match tool.kind {
        Some(
            ToolKind::Read | ToolKind::Search | ToolKind::Edit | ToolKind::Delete | ToolKind::Move,
        ) => !tool.locations.is_empty() && all().all(|path| inside(copy, path)),
        _ => false,
    }
}

/// The option allowing the question once, if the agent offers one.
pub(super) fn allow_once(asked: &RequestPermissionRequest) -> Option<PermissionOptionId> {
    asked
        .options
        .iter()
        .find(|o| o.kind == PermissionOptionKind::AllowOnce)
        .map(|o| o.option_id.clone())
}

/// What a card shows of a tool call: its command, else the paths it touches (project paths inside the copy), else
/// the files | address its input names; one per line.
pub(super) fn detail(tool: &ToolLine, copy: &Vault) -> String {
    let input = tool.raw_input.as_ref();
    match input.and_then(|raw| raw.get("command")) {
        Some(Value::String(command)) => return command.clone(),
        Some(Value::Array(parts)) => {
            return parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        }
        _ => {}
    }
    let shown = |path: &Path| match copy.relativize(path) {
        Some(relative) if inside(copy, path) && !relative.is_root() => relative.as_str().to_owned(),
        _ => path.display().to_string(),
    };
    let paths = if tool.locations.is_empty() {
        tool.input_paths(copy)
    } else {
        tool.locations.clone()
    };
    let mut lines: Vec<String> = paths.iter().map(|path| shown(path)).collect();
    if lines.is_empty() {
        lines.extend(
            input
                .and_then(|raw| raw.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        );
    }
    lines.join("\n")
}
