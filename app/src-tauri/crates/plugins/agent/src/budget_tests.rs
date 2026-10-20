//! Token budget of every tool: the committed fixture project, one typical call per tool, each answer measured with
//! the core estimator against the ceilings in `budgets.txt` (more than 10 % over fails).

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use permark_core::conditions::Distribution;
use permark_core::events::NoEvents;
use permark_core::notes::{SharedNote, SharedNotes, SharedNotesApi};
use permark_core::organizing::{
    CommentBook, CommentFilter, CommentItem, CommentsApi, TagCount, TagLookup, TagsApi,
    TagsSummary, UnreadApi, UnreadLookup, UnreadNow,
};
use permark_core::paths::Root;
use permark_core::plugins::{CxWorkspace, PluginCx, PluginId, PluginRegistry, ServiceLease};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::tokens::estimate;
use permark_core::workspace::{CoreEnv, ProjectCore};
use permark_core::IpcError;
use serde_json::{json, Value};

use crate::context::{Mode, Section, Selection, WindowContext};
use crate::live::Live;
use crate::perms::{ProjectChoice, PROJECT_FILE};
use crate::tools::Tools;

/// The fixture project (committed with the sources).
fn fixture_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../testdata/agent-fixture")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            // the same bytes on every OS (a checkout may turn line endings into CRLF)
            let text = std::fs::read_to_string(entry.path()).unwrap();
            std::fs::write(target, text.replace("\r\n", "\n")).unwrap();
        }
    }
}

struct Tags;

impl TagLookup for Tags {
    fn files_with_all(&self, tags: &[String]) -> HashSet<String> {
        let mut files = HashSet::new();
        for (path, have) in [
            ("README.md", &["overview", "plan"][..]),
            ("docs/plan.md", &["plan"][..]),
            ("private/secret.md", &["plan"][..]),
        ] {
            if tags.iter().all(|t| have.contains(&t.as_str())) {
                files.insert(path.to_owned());
            }
        }
        files
    }
    fn tags_of(&self, path: &str) -> Vec<String> {
        match path {
            "README.md" => vec!["overview".to_owned(), "plan".to_owned()],
            "docs/plan.md" | "private/secret.md" => vec!["plan".to_owned()],
            _ => Vec::new(),
        }
    }
    fn summary(&self) -> TagsSummary {
        TagsSummary {
            tags: vec![
                TagCount {
                    tag: "plan".to_owned(),
                    count: 3,
                },
                TagCount {
                    tag: "overview".to_owned(),
                    count: 1,
                },
            ],
            documents: 8,
            untagged: 5,
        }
    }
    fn description_of(&self, path: &str) -> String {
        if path == "README.md" {
            "Project overview for the Atlas sync service".to_owned()
        } else {
            String::new()
        }
    }
    fn wait_ready(&self, _: Duration) -> bool {
        true
    }
}

struct Comments(Mutex<Vec<CommentItem>>);

impl CommentBook for Comments {
    fn list(&self, which: CommentFilter) -> Result<Vec<CommentItem>, IpcError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|c| match which {
                CommentFilter::Open => !c.done,
                CommentFilter::Done => c.done,
                CommentFilter::All => true,
            })
            .cloned()
            .collect())
    }
    fn resolve(&self, id: &str, reply: Option<&str>) -> Result<(), IpcError> {
        let mut list = self.0.lock().unwrap();
        if let Some(found) = list.iter_mut().find(|c| c.id == id) {
            found.done = true;
            found.reply = reply.map(str::to_owned);
        }
        Ok(())
    }
}

fn comment(
    id: &str,
    path: &str,
    heading: &[&str],
    line: u32,
    quote: &str,
    text: &str,
) -> CommentItem {
    CommentItem {
        id: id.to_owned(),
        path: path.to_owned(),
        quote: quote.to_owned(),
        text: text.to_owned(),
        heading_path: heading.iter().map(|h| (*h).to_owned()).collect(),
        line,
        state: "anchored",
        created_ms: 1_760_000_000_000,
        done: false,
        reply: None,
    }
}

struct Unread;

impl UnreadLookup for Unread {
    fn unread(&self) -> UnreadNow {
        UnreadNow {
            ready: true,
            paths: ["README.md", "docs/notes/meeting-a.md", "docs/spec.md"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        }
    }
    fn wait_ready(&self, _: Duration) -> bool {
        true
    }
}

struct Notes;

impl SharedNotes for Notes {
    fn list(&self) -> Vec<SharedNote> {
        vec![
            SharedNote {
                id: "2026-10-15 1432.md".to_owned(),
                title: "Deploy checklist".to_owned(),
                updated_ms: 1_760_000_000_000,
            },
            SharedNote {
                id: "2026-10-16 0910.md".to_owned(),
                title: "Retry ideas".to_owned(),
                updated_ms: 1_760_100_000_000,
            },
        ]
    }
    fn read(&self, id: &str) -> Result<String, IpcError> {
        Ok(format!(
            "# {id}\n\n- Check the retry policy.\n- Ask about the release notes.\n"
        ))
    }
}

pub(crate) struct Budget {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    core: Option<Arc<ProjectCore>>,
    _env: Arc<CoreEnv>,
    _leases: Vec<ServiceLease>,
}

impl Drop for Budget {
    fn drop(&mut self) {
        self.core.take();
    }
}

impl Budget {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("pmb").tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap().join("atlas");
        copy_dir(&fixture_source(), &root);
        let env = Arc::new(CoreEnv::new(
            AppDirs::under(&dir.path().join("state")),
            Arc::new(PluginRegistry::new(vec![crate::plugin()]).unwrap()),
            Arc::new(NoEvents),
            Distribution::Direct,
            AppSettings::default(),
        ));
        let core = ProjectCore::open(Root::open(&root).unwrap(), &env);
        let services = core.services();
        let comments = Comments(Mutex::new(vec![
            comment(
                "c1",
                "docs/plan.md",
                &["Plan", "Rollout"],
                17,
                "Ship the beta on Friday",
                "Which Friday? Add the date.",
            ),
            comment(
                "c2",
                "docs/spec.md",
                &["Spec", "Retry policy"],
                9,
                "waits 2, 4, 8 and then 16 seconds",
                "Is 16 seconds enough after a long outage?",
            ),
            comment(
                "c3",
                "README.md",
                &["Atlas", "Getting started"],
                12,
                "Install the service.",
                "Link the install page.",
            ),
        ]));
        let leases = vec![
            services.services.provide(Arc::new(TagsApi(Arc::new(Tags)))),
            services
                .services
                .provide(Arc::new(CommentsApi(Arc::new(comments)))),
            services
                .services
                .provide(Arc::new(UnreadApi(Arc::new(Unread)))),
            env.services()
                .provide(Arc::new(SharedNotesApi(Arc::new(Notes)))),
        ];
        assert!(core.search().wait_ready(Duration::from_secs(10)));
        let mut groups = BTreeMap::new();
        groups.insert("comments.resolve".to_owned(), true);
        core.store()
            .profile_file::<ProjectChoice>(PROJECT_FILE)
            .save(&ProjectChoice {
                enabled: true,
                groups,
                resolve_asked: false,
            })
            .unwrap();
        Self {
            _dir: dir,
            root,
            core: Some(core),
            _env: env,
            _leases: leases,
        }
    }

    pub(crate) fn core(&self) -> &Arc<ProjectCore> {
        self.core.as_ref().unwrap()
    }

    pub fn tools(&self) -> Tools {
        let cx = PluginCx::new(
            PluginId::of("agent"),
            CxWorkspace::Project(self.core().vault().clone()),
            self.root.clone(),
            None,
            permark_core::conditions::Context::welcome(Distribution::Direct),
            Arc::new(NoEvents),
        )
        .with_project(self.core().services());
        let live = Arc::new(Live::new(cx.events().clone()));
        live.publish(WindowContext {
            doc: Some("docs/plan.md".to_owned()),
            mode: Mode::View,
            section: Some(Section {
                heading_path: vec!["Plan".to_owned(), "Rollout".to_owned()],
                lines: Some((15, 25)),
            }),
            selection: Some(Selection {
                text: "Ship the beta on Friday to the first ten teams.".to_owned(),
                lines: Some((17, 17)),
                cut: false,
            }),
            unsaved: false,
        });
        Tools::new(
            self.core().vault().clone(),
            self.core().store().clone(),
            self.core().services(),
            Some(live),
        )
    }
}

/// The typical call of each tool (and two variants), by the name its ceiling has in `budgets.txt`.
pub(crate) fn typical_calls() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("get_context", "get_context", json!({})),
        ("list_unread", "list_unread", json!({})),
        ("map", "map", json!({})),
        ("search", "search", json!({ "query": "retry policy" })),
        (
            "search+budget",
            "search",
            json!({ "query": "retry policy", "budget": 200 }),
        ),
        ("list_tags", "list_tags", json!({})),
        ("outline", "outline", json!({ "path": "docs/plan.md" })),
        (
            "outline+links",
            "outline",
            json!({ "path": "docs/spec.md", "links": true }),
        ),
        (
            "read",
            "read",
            json!({ "path": "docs/plan.md", "at": "Plan > Rollout" }),
        ),
        ("list_comments", "list_comments", json!({})),
        (
            "resolve_comment",
            "resolve_comment",
            json!({ "id": "c3", "reply": "Linked." }),
        ),
        ("list_notes", "list_notes", json!({})),
        (
            "read_note",
            "read_note",
            json!({ "note": "2026-10-15 1432.md" }),
        ),
        (
            "open",
            "open",
            json!({ "path": "docs/spec.md", "heading": "Retry policy" }),
        ),
        ("activity", "activity", json!({})),
    ]
}

/// What the person did before the hand-off the `activity` call reads.
fn hand_off(fx: &Budget, tools: &Tools) {
    use permark_core::activity::{CommentOp, Fact};
    let journal = &fx.core().services().activity;
    for fact in [
        Fact::Save {
            path: "docs/plan.md".to_owned(),
            sections: vec!["Plan > Rollout".to_owned(), "Plan > Risks".to_owned()],
            blocks: 2,
        },
        Fact::Tag {
            added: true,
            path: "docs/spec.md".to_owned(),
            tag: "protocol".to_owned(),
        },
        Fact::Comment {
            op: CommentOp::Add,
            path: "docs/plan.md".to_owned(),
            id: "c1".to_owned(),
            heading: Some("Plan > Rollout".to_owned()),
        },
        Fact::Convert {
            path: "data/items.json".to_owned(),
            to: "toon".to_owned(),
            saved: Some("data/items.toon".to_owned()),
        },
    ] {
        journal.person(fact);
    }
    let guard = tools.guard_now();
    let crate::handoff::HandOffPreview::Ready { upto, sessions, .. } =
        crate::handoff::preview(tools, &guard, None)
    else {
        panic!("a preview");
    };
    let checked: Vec<u32> = sessions
        .iter()
        .filter(|s| s.checked)
        .map(|s| s.id)
        .collect();
    crate::handoff::confirm(tools, &guard, upto, &checked).unwrap();
}

/// The committed ceilings: `name<TAB>tokens` per line.
fn ceilings() -> BTreeMap<String, u64> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("budgets.txt");
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let (name, n) = l.split_once('\t')?;
            Some((name.to_owned(), n.trim().parse().ok()?))
        })
        .collect()
}

/// Every typical answer fits its ceiling (+10 %); `PERMARK_BUDGETS=write` writes the ceilings from this run.
#[test]
fn every_tool_answer_stays_within_its_token_budget() {
    let fx = Budget::new();
    let tools = fx.tools();
    hand_off(&fx, &tools);
    let mut measured: Vec<(String, u64, usize)> = Vec::new();
    for (name, tool, args) in typical_calls() {
        let value = tools
            .call(tool, args)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let text = crate::render::render(tool, &value);
        assert!(
            text.chars().count() <= crate::format::CEILING_CHARS,
            "{name}"
        );
        measured.push((name.to_owned(), estimate(&text), text.len()));
        println!(
            "=== {name} ~{} tokens, {} bytes\n{text}",
            estimate(&text),
            text.len()
        );
    }
    if std::env::var("PERMARK_BUDGETS").as_deref() == Ok("write") {
        let mut out = String::from(
            "# Token ceilings of the typical answer of each agent tool (core estimator, fixture testdata/agent-fixture).\n# More than 10 % over fails; lower them when an answer gets smaller.\n",
        );
        for (name, tokens, _) in &measured {
            let _ = writeln!(out, "{name}\t{tokens}");
        }
        std::fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("budgets.txt"),
            out,
        )
        .unwrap();
        return;
    }
    let ceilings = ceilings();
    for (name, tokens, _) in &measured {
        let ceiling = *ceilings
            .get(name)
            .unwrap_or_else(|| panic!("{name}: no ceiling in budgets.txt"));
        // integer math: more than 10 % over the committed ceiling fails
        assert!(
            tokens * 10 <= ceiling * 11,
            "{name}: ~{tokens} tokens, ceiling {ceiling} (+10 %)"
        );
    }
}
