//! What each agent tool does, with no MCP and no socket in it: [`Tools::call`] checks the switch, the group and the
//! paths, then answers from the project's services. The app's server and `permark mcp` with the app closed run this
//! same code over the same composition (`ProjectServices`).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use permark_core::fsx;
use permark_core::notes::SharedNotesApi;
use permark_core::organizing::{CommentFilter, CommentsApi, SilenceApi, TagsApi, UnreadApi};
use permark_core::paths::Vault;
use permark_core::plugins::ProjectServices;
use permark_core::search::SearchFilter;
use permark_core::secrets::mask_text;
use permark_core::settings::ProjectConfig;
use permark_core::state::StateStore;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::context::WindowContext;
use crate::guard::Guard;
use crate::live::Live;
use crate::perms::{self, Access, Group};
use crate::structure;
use crate::ToolError;

/// How long a tool waits for a first look at the project (index, tags, unread) before answering partially.
pub const READY_WAIT: Duration = Duration::from_secs(10);
/// Largest file `outline` | `read` open.
pub const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
/// Longest text one `read` returns (characters); the answer says where to go on.
pub const MAX_READ_CHARS: usize = 20_000;

/// The tools, their groups, in the order agents list them.
pub const TOOLS: [(&str, Group); 11] = [
    ("get_context", Group::Context),
    ("list_unread", Group::Changes),
    ("search", Group::Find),
    ("list_tags", Group::Find),
    ("outline", Group::Find),
    ("read", Group::Find),
    ("list_comments", Group::CommentsRead),
    ("resolve_comment", Group::CommentsResolve),
    ("list_notes", Group::Notes),
    ("read_note", Group::Notes),
    ("open", Group::Show),
];

#[must_use]
pub fn group_of(tool: &str) -> Option<Group> {
    TOOLS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, g)| *g)
}

/// One project as the tools see it.
pub struct Tools {
    vault: Vault,
    store: StateStore,
    services: ProjectServices,
    /// The window's half (`get_context`, `open`): only inside the app.
    live: Option<Arc<Live>>,
}

fn args<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ToolError> {
    let value = if value.is_null() { json!({}) } else { value };
    serde_json::from_value(value).map_err(|e| ToolError::Invalid(e.to_string()))
}

/// `2026-10-15T09:30:00Z` for milliseconds since the epoch.
#[must_use]
pub fn iso(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn modified_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn limit(asked: Option<usize>, default: usize, max: usize) -> usize {
    asked.unwrap_or(default).clamp(1, max)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitArgs {
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    query: Option<String>,
    limit: Option<usize>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagArgs {
    tag: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
    at: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WhichArgs {
    which: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveArgs {
    id: String,
    reply: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteArgs {
    note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenArgs {
    path: String,
    line: Option<u32>,
    heading: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

/// Where an agent asked to show the person something (checked: a shared file of the project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: String,
    pub line: Option<u32>,
    pub heading: Option<String>,
}

impl Tools {
    #[must_use]
    pub fn new(
        vault: Vault,
        store: StateStore,
        services: ProjectServices,
        live: Option<Arc<Live>>,
    ) -> Self {
        Self {
            vault,
            store,
            services,
            live,
        }
    }

    #[must_use]
    pub fn store(&self) -> &StateStore {
        &self.store
    }

    #[must_use]
    pub fn vault_root(&self) -> &permark_core::paths::Root {
        self.vault.main()
    }

    /// The rules of a call made now (for `permark context` and the published context file).
    #[must_use]
    pub fn guard_now(&self) -> Arc<Guard> {
        self.guard(&ProjectConfig::load(self.vault.main()))
    }

    /// The rules of one call: `.aiignore`, `[agent] deny_paths`, silenced paths.
    fn guard(&self, config: &ProjectConfig) -> Arc<Guard> {
        Arc::new(Guard::new(
            self.vault.main(),
            &config.agent_deny_paths,
            self.services
                .services
                .get::<SilenceApi>()
                .map(|a| (*a).clone()),
        ))
    }

    /// Checks the switch and the group, then runs the tool.
    ///
    /// # Errors
    /// `Off`, `Denied`, `Invalid`, `NotFound`, `NotOpen`, `Failed`.
    pub fn call(&self, tool: &str, args: Value) -> Result<Value, ToolError> {
        let group =
            group_of(tool).ok_or_else(|| ToolError::Invalid(format!("unknown tool {tool}")))?;
        let choice = perms::load(&self.store);
        if !choice.enabled {
            return Err(ToolError::Off);
        }
        let config = ProjectConfig::load(self.vault.main());
        let access = perms::effective(&choice.groups, &config.agent_deny)
            .get(&group)
            .copied()
            .unwrap_or(Access::Off);
        if access != Access::On {
            return Err(ToolError::Denied {
                group,
                first_ask: group == Group::CommentsResolve
                    && access == Access::Off
                    && !choice.resolve_asked,
            });
        }
        let guard = self.guard(&config);
        match tool {
            "get_context" => self.get_context(args, &guard),
            "list_unread" => self.list_unread(args, &guard),
            "search" => self.search(args, &guard),
            "list_tags" => self.list_tags(args, &guard),
            "outline" => self.outline(args, &guard),
            "read" => self.read(args, &guard),
            "list_comments" => self.list_comments(args, &guard),
            "resolve_comment" => self.resolve_comment(args, &guard),
            "list_notes" => self.list_notes(args),
            "read_note" => self.read_note(args),
            "open" => self.open(args, &guard),
            other => Err(ToolError::Invalid(format!("unknown tool {other}"))),
        }
    }

    fn open_comments(&self, guard: &Guard) -> usize {
        self.services
            .services
            .get::<CommentsApi>()
            .and_then(|api| api.0.list(CommentFilter::Open).ok())
            .map_or(0, |list| {
                list.iter()
                    .filter(|c| !guard.hidden(&c.path, false))
                    .count()
            })
    }

    /// What the person is looking at (sanitized again: the rules may have changed since it was published).
    ///
    /// # Errors
    /// `NotOpen` without the app.
    pub fn context(&self, guard: &Guard) -> Result<(WindowContext, usize), ToolError> {
        let live = self.live.as_ref().ok_or(ToolError::NotOpen)?;
        Ok((live.context().sanitize(guard), self.open_comments(guard)))
    }

    fn get_context(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let NoArgs {} = args(raw)?;
        let (context, open) = self.context(guard)?;
        let mut value =
            serde_json::to_value(&context).map_err(|e| ToolError::Failed(e.to_string()))?;
        if let Some(map) = value.as_object_mut() {
            map.retain(|_, v| !v.is_null());
            map.insert("openComments".to_owned(), json!(open));
        }
        Ok(value)
    }

    fn list_unread(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let LimitArgs { limit: asked } = args(raw)?;
        let api = self.services.services.get::<UnreadApi>().ok_or_else(|| {
            ToolError::Invalid("unread markers are off in this project".to_owned())
        })?;
        let ready = api.0.wait_ready(READY_WAIT);
        let now = api.0.unread();
        let mut items: Vec<(String, u64)> = now
            .paths
            .into_iter()
            .filter(|p| !guard.hidden(p, false))
            .filter_map(|p| guard.file(&p).ok())
            .map(|(p, abs)| {
                let ms = modified_ms(&abs);
                (p, ms)
            })
            .collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let max = limit(asked, 50, 200);
        let more = items.len().saturating_sub(max);
        items.truncate(max);
        Ok(json!({
            "ready": ready && now.ready,
            "unread": items.iter().map(|(p, ms)| json!({ "path": p, "modified": iso(*ms) })).collect::<Vec<_>>(),
            "more": more,
        }))
    }

    fn tags_api(&self) -> Result<Arc<TagsApi>, ToolError> {
        let api = self
            .services
            .services
            .get::<TagsApi>()
            .ok_or_else(|| ToolError::Invalid("tags are off in this project".to_owned()))?;
        api.0.wait_ready(READY_WAIT);
        Ok(api)
    }

    fn tagged(&self, tags: &[String], guard: &Guard) -> Result<Vec<String>, ToolError> {
        let api = self.tags_api()?;
        let mut paths: Vec<String> = api
            .0
            .files_with_all(tags)
            .into_iter()
            .filter(|p| !guard.hidden(p, false))
            .collect();
        paths.sort_by_key(|p| p.to_lowercase());
        Ok(paths)
    }

    fn file_entry(&self, path: &str) -> Value {
        let (description, tags) = self.tags_api().map_or_else(
            |_| (String::new(), Vec::new()),
            |api| (api.0.description_of(path), api.0.tags_of(path)),
        );
        let mut entry = Map::new();
        entry.insert("path".to_owned(), json!(path));
        if !description.is_empty() {
            entry.insert("description".to_owned(), json!(mask_text(&description)));
        }
        entry.insert("tags".to_owned(), json!(tags));
        Value::Object(entry)
    }

    fn search(&self, raw: Value, guard: &Arc<Guard>) -> Result<Value, ToolError> {
        let SearchArgs {
            query,
            limit: asked,
            tags,
        } = args(raw)?;
        let query = query.unwrap_or_default();
        let tags: Vec<String> = tags
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect();
        if query.trim().is_empty() {
            if tags.is_empty() {
                return Err(ToolError::Invalid("give a query, tags, or both".to_owned()));
            }
            let paths = self.tagged(&tags, guard)?;
            let max = limit(asked, 20, 100);
            return Ok(json!({
                "documents": paths.iter().take(max).map(|p| self.file_entry(p)).collect::<Vec<_>>(),
                "total": paths.len(),
                "more": paths.len().saturating_sub(max),
            }));
        }
        let only = if tags.is_empty() {
            None
        } else {
            Some(self.tagged(&tags, guard)?.into_iter().collect())
        };
        let filter = SearchFilter {
            only,
            skip: std::collections::HashSet::new(),
            muted: Some(guard.as_lookup()),
        };
        let index = &self.services.search;
        let ready = index.wait_ready(READY_WAIT);
        let results = index.sections(query.trim(), limit(asked, 10, 25), &filter);
        let hits: Vec<Value> = results
            .hits
            .into_iter()
            .map(|hit| {
                let snippet: String = hit.snippet.iter().map(|p| p.text.as_str()).collect();
                json!({
                    "path": hit.path,
                    "headingPath": hit.heading_path,
                    "line": hit.line,
                    "snippet": mask_text(snippet.trim()),
                })
            })
            .collect();
        Ok(json!({ "hits": hits, "partial": results.partial || !ready }))
    }

    fn list_tags(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let TagArgs { tag, limit: asked } = args(raw)?;
        let api = self.tags_api()?;
        if let Some(tag) = tag.filter(|t| !t.trim().is_empty()) {
            let paths = self.tagged(&[tag.trim().to_owned()], guard)?;
            let max = limit(asked, 20, 100);
            return Ok(json!({
                "tag": tag.trim(),
                "files": paths.iter().take(max).map(|p| self.file_entry(p)).collect::<Vec<_>>(),
                "total": paths.len(),
                "more": paths.len().saturating_sub(max),
            }));
        }
        let summary = api.0.summary();
        // counts without the files agents do not get
        let mut tags: Vec<(String, usize)> = summary
            .tags
            .into_iter()
            .map(|t| {
                let count = api
                    .0
                    .files_with_all(std::slice::from_ref(&t.tag))
                    .iter()
                    .filter(|p| !guard.hidden(p, false))
                    .count();
                (t.tag, count)
            })
            .filter(|(_, count)| *count > 0)
            .collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let max = limit(asked, 50, 200);
        let more = tags.len().saturating_sub(max);
        tags.truncate(max);
        Ok(json!({
            "tags": tags.iter().map(|(tag, count)| json!({ "tag": tag, "count": count })).collect::<Vec<_>>(),
            "documents": summary.documents,
            "untagged": summary.untagged,
            "more": more,
        }))
    }

    fn comments(&self) -> Result<Arc<CommentsApi>, ToolError> {
        self.services
            .services
            .get::<CommentsApi>()
            .ok_or_else(|| ToolError::Invalid("comments are off in this project".to_owned()))
    }

    fn list_comments(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let WhichArgs { which } = args(raw)?;
        let filter = match which.as_deref().map(str::trim) {
            None | Some("" | "open") => CommentFilter::Open,
            Some("done") => CommentFilter::Done,
            Some("all") => CommentFilter::All,
            Some(other) => {
                return Err(ToolError::Invalid(format!(
                    "which is open, done or all, not {other:?}"
                )))
            }
        };
        let list = self.comments()?.0.list(filter).map_err(|e| {
            ToolError::Failed(format!("the comments cannot be read now: {}", why(&e)))
        })?;
        let comments: Vec<Value> = list
            .into_iter()
            .filter(|c| !guard.hidden(&c.path, false))
            .map(|c| {
                let mut item = json!({
                    "id": c.id,
                    "path": c.path,
                    "quote": mask_text(&c.quote),
                    "text": mask_text(&c.text),
                    "headingPath": c.heading_path,
                    "line": c.line,
                    "state": c.state,
                    "created": iso(c.created_ms),
                    "done": c.done,
                });
                if let (Some(reply), Some(map)) = (c.reply, item.as_object_mut()) {
                    map.insert("reply".to_owned(), json!(mask_text(&reply)));
                }
                item
            })
            .collect();
        Ok(json!({ "comments": comments }))
    }

    fn resolve_comment(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let ResolveArgs { id, reply } = args(raw)?;
        let api = self.comments()?;
        let list = api.0.list(CommentFilter::All).map_err(|e| {
            ToolError::Failed(format!("the comments cannot be read now: {}", why(&e)))
        })?;
        // a comment on a file agents do not get is not there for them
        if !list
            .iter()
            .any(|c| c.id == id && !guard.hidden(&c.path, false))
        {
            return Err(ToolError::NotFound(id));
        }
        api.0
            .resolve(&id, reply.as_deref())
            .map_err(|e| ToolError::Failed(format!("not marked done: {}", why(&e))))?;
        Ok(json!({ "resolved": id }))
    }

    fn notes(&self) -> Result<Arc<SharedNotesApi>, ToolError> {
        self.services
            .app_services
            .get::<SharedNotesApi>()
            .ok_or_else(|| ToolError::Invalid("notes are off".to_owned()))
    }

    fn list_notes(&self, raw: Value) -> Result<Value, ToolError> {
        let NoArgs {} = args(raw)?;
        let notes: Vec<Value> = self
            .notes()?
            .0
            .list()
            .into_iter()
            .map(|n| json!({ "note": n.id, "title": mask_text(&n.title), "updated": iso(n.updated_ms) }))
            .collect();
        Ok(json!({ "notes": notes }))
    }

    fn read_note(&self, raw: Value) -> Result<Value, ToolError> {
        let NoteArgs { note } = args(raw)?;
        let api = self.notes()?;
        let shared = api.0.list();
        let Some(found) = shared.iter().find(|n| n.id == note) else {
            return Err(ToolError::NotFound(note));
        };
        let text = api
            .0
            .read(&note)
            .map_err(|_| ToolError::NotFound(note.clone()))?;
        Ok(json!({ "note": note, "title": mask_text(&found.title), "text": mask_text(&text) }))
    }

    fn outline(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let PathArgs { path } = args(raw)?;
        let (path, abs) = guard.file(&path)?;
        let text = read_text(&abs)?;
        Ok(structure::outline(&self.services.formats(), &path, &text))
    }

    fn read(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let ReadArgs { path, at } = args(raw)?;
        let (path, abs) = guard.file(&path)?;
        let text = read_text(&abs)?;
        structure::read(&self.services.formats(), &path, &text, at.as_deref())
    }

    fn open(&self, raw: Value, guard: &Guard) -> Result<Value, ToolError> {
        let OpenArgs {
            path,
            line,
            heading,
        } = args(raw)?;
        let (path, abs) = guard.file(&path)?;
        let heading = heading
            .map(|h| h.trim().to_owned())
            .filter(|h| !h.is_empty());
        // a heading wins over a line when the document has it; the window gets a line either way
        let found = heading.as_deref().and_then(|h| {
            read_text(&abs)
                .ok()
                .and_then(|text| structure::heading_line(&path, &text, h))
        });
        let live = self.live.as_ref().ok_or(ToolError::NotOpen)?;
        let place = Place {
            path,
            line: found.or(line.filter(|l| *l >= 1)),
            heading: heading.clone(),
        };
        let shown = live.show(&place);
        let mut answer = json!({ "path": place.path, "status": shown });
        if let (Some(map), Some(_), None) = (answer.as_object_mut(), &heading, found) {
            map.insert("headingNotFound".to_owned(), json!(true));
        }
        Ok(answer)
    }
}

fn read_text(abs: &Path) -> Result<String, ToolError> {
    match fsx::read_text(abs, MAX_READ_BYTES) {
        Ok(decoded) => Ok(decoded.text),
        Err(fsx::FsxError::TooLarge { .. }) => Err(ToolError::Invalid("too large".to_owned())),
        Err(fsx::FsxError::Binary(_)) => Err(ToolError::Invalid("not a text file".to_owned())),
        Err(fsx::FsxError::Io { .. }) => {
            Err(ToolError::Failed("cannot read the file now".to_owned()))
        }
    }
}

fn why(error: &permark_core::IpcError) -> String {
    format!("{:?}", error.kind)
}
