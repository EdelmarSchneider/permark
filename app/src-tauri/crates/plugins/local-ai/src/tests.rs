use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, RwLock};

use permark_core::conditions::{Context, Distribution};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{AppServices, CxWorkspace, RpcCall, RpcTable, Services};
use permark_core::settings::AppSettings;
use permark_core::state::{AppDirs, StateStore};
use serde::Deserialize;
use serde_json::{json, Value};

use super::*;
use crate::catalog::{FileRole, PUBLISHERS};

mod host;
mod installs;
mod pieces;
mod store;
mod testrun;

/// Shared with vitest (`app/src/plugins/local-ai/tasks.test.ts`): both mirrors give the same answers.
const TASKS_FIXTURE: &str = include_str!("../../../../../src/plugins/local-ai/tasks.json");

#[derive(Default)]
struct Events(Mutex<Vec<String>>);

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, .. } = event {
            self.0.lock().unwrap().push(name);
        }
    }
}

struct Harness {
    tmp: tempfile::TempDir,
    rpc: Arc<RpcTable>,
    running: Option<Box<dyn Running>>,
}

impl Harness {
    fn start() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(tmp.path());
        let rpc = RpcTable::new();
        let store = StateStore::app(&dirs);
        let cx = PluginCx::new(
            ID,
            CxWorkspace::App,
            store.plugin_dir(&ID),
            None,
            Context::welcome(Distribution::Direct),
            Arc::new(Events::default()) as Arc<dyn EventSink>,
        )
        .with_store(store)
        .with_app(AppServices::new(
            Arc::clone(&rpc),
            Services::new(),
            Arc::new(RwLock::new(AppSettings::default())),
        ));
        let running = plugin().start(&cx).unwrap();
        Self {
            tmp,
            rpc,
            running: Some(running),
        }
    }

    fn call(&self, method: &str) -> Result<Value, permark_core::IpcError> {
        self.rpc.call(
            "local-ai",
            method,
            &RpcCall {
                window: "main".into(),
                args: json!({}),
            },
        )
    }
}

/// Every entry under `dir` (recursive), relative.
pub(crate) fn entries(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            out.push(path.strip_prefix(dir).unwrap().display().to_string());
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    out
}

#[test]
fn the_manifest_is_an_app_feature_that_starts_on() {
    let manifest = plugin().manifest().clone();
    assert_eq!(manifest.id.as_str(), "local-ai");
    assert_eq!(manifest.kind, PluginKind::Feature);
    assert_eq!(manifest.scope, PluginScope::App);
    assert_eq!(manifest.requires, Requirement::NONE);
    assert!(manifest.default_enabled);
    assert!(manifest.depends_on.is_empty());
}

/// AC-1: on but dormant -> starting and serving the read-only RPC creates no file | folder (profile, cache).
#[test]
fn starting_and_answering_creates_nothing_on_disk() {
    let mut h = Harness::start();
    h.call("catalog").unwrap();
    h.call("tasks").unwrap();
    h.running = None;
    assert_eq!(entries(h.tmp.path()), Vec::<String>::new());
}

/// AC-9: the machine RPC answers the profile, a fit per catalog model, no budget warning (nothing assigned); it
/// reads only (no file | folder).
#[test]
fn the_machine_rpc_advises_on_every_model_and_creates_nothing() {
    let mut h = Harness::start();
    let info = h.call("machine").unwrap();
    assert_eq!(info["profile"]["os"], std::env::consts::OS);
    let fits = info["fits"].as_object().unwrap();
    let ids: BTreeSet<&str> = fits.keys().map(String::as_str).collect();
    let catalog: BTreeSet<&str> = catalog::bundled()
        .unwrap()
        .models
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(ids, catalog);
    for fit in fits.values() {
        assert!(
            ["runsWell", "slowCpuOnly", "notRecommended", "unknown"]
                .contains(&fit.as_str().unwrap()),
            "{fit}"
        );
    }
    assert_eq!(info["budget"], Value::Null);
    h.running = None;
    assert!(h.call("machine").is_err());
    assert_eq!(entries(h.tmp.path()), Vec::<String>::new());
}

/// Off -> nothing left: the RPC goes with the running plugin.
#[test]
fn stopping_removes_the_rpc() {
    let mut h = Harness::start();
    assert!(h.call("catalog").is_ok());
    h.running = None;
    assert!(h.call("catalog").is_err());
    assert!(h.call("tasks").is_err());
}

#[test]
fn the_catalog_rpc_lists_every_bundled_model_for_the_windows() {
    let h = Harness::start();
    let answer = h.call("catalog").unwrap();
    let bundled = catalog::bundled().unwrap();
    assert_eq!(answer["schemaVersion"], catalog::SCHEMA_VERSION);
    let models = answer["models"].as_array().unwrap();
    assert_eq!(models.len(), bundled.models.len());
    let lfm = models
        .iter()
        .find(|m| m["id"] == "lfm3-1.7b-q4_k_m")
        .unwrap();
    assert_eq!(lfm["license"]["needsAcceptance"], true);
    assert_eq!(lfm["bytes"], 1_099_627_808_u64);
    assert_eq!(lfm["capabilities"], json!(["chat", "tools"]));
    assert_eq!(lfm["gated"], false);
    let e2b = models
        .iter()
        .find(|m| m["id"] == "gemma-4-e2b-it-q4_0")
        .unwrap();
    assert_eq!(e2b["bytes"], 2_841_481_184_u64 + 557_368_064);
}

#[test]
fn the_tasks_rpc_lists_every_task_with_its_needs() {
    let h = Harness::start();
    let answer = h.call("tasks").unwrap();
    assert_eq!(answer, serde_json::to_value(tasks::infos()).unwrap());
    assert_eq!(
        answer[5],
        json!({"task": "assistant", "needs": [["chat", "tools"]]})
    );
}

// ---- catalog (AC-2)

#[test]
fn the_bundled_catalog_parses_and_holds_the_initial_models() {
    let catalog = catalog::bundled().unwrap();
    let ids: BTreeSet<_> = catalog.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "embeddinggemma-2-q8_0",
            "gemma-4-e2b-it-q4_0",
            "gemma-4-e4b-it-q4_0",
            "lfm3-1.7b-q4_k_m",
            "whisper-large-v3-turbo-q5_0",
        ])
    );
    assert_eq!(ids.len(), catalog.models.len(), "ids unique");
}

#[test]
fn every_model_is_pinned_from_an_allowed_publisher_at_a_commit() {
    for model in &catalog::bundled().unwrap().models {
        assert!(
            PUBLISHERS.contains(&model.publisher.as_str()),
            "{}",
            model.id
        );
        assert!(model.repo.starts_with(&format!("{}/", model.publisher)));
        assert_eq!(model.commit.len(), 40);
        assert!(model.commit.bytes().all(|b| b.is_ascii_hexdigit()));
        for file in &model.files {
            assert!(file.bytes > 0, "{} {}", model.id, file.path);
            assert_eq!(file.sha256.len(), 64);
            assert!(file.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        assert!(model.files.iter().any(|f| f.role == FileRole::Weights));
    }
}

#[test]
fn every_file_is_asked_at_the_pinned_commit_on_hugging_face() {
    for model in &catalog::bundled().unwrap().models {
        let assets = model.assets().unwrap();
        assert_eq!(assets.len(), model.files.len());
        for (asset, file) in assets.iter().zip(&model.files) {
            assert_eq!(
                asset.url(),
                format!(
                    "https://huggingface.co/{}/resolve/{}/{}",
                    model.repo, model.commit, file.path
                )
            );
            assert_eq!(asset.bytes(), file.bytes);
            assert_eq!(asset.revision(), Some(model.commit.as_str()));
            assert_eq!(asset.name(), file.path);
        }
    }
}

#[test]
fn needs_follow_the_documented_rule() {
    for model in &catalog::bundled().unwrap().models {
        let needs = model.needs;
        assert!(needs.ram_min >= model.bytes(), "{}", model.id);
        assert!(needs.ram_rec >= needs.ram_min);
        assert!(needs.vram_rec >= model.bytes());
    }
}

#[test]
fn licenses_that_ask_for_acceptance_say_so() {
    let catalog = catalog::bundled().unwrap();
    let lfm = catalog.model("lfm3-1.7b-q4_k_m").unwrap();
    assert!(lfm.license.needs_acceptance);
    assert_eq!(lfm.license.revision.as_deref(), Some("v1.0"));
    assert!(lfm.license.restriction.is_some());
    for id in ["gemma-4-e2b-it-q4_0", "embeddinggemma-2-q8_0"] {
        let model = catalog.model(id).unwrap();
        assert_eq!(model.license.spdx.as_deref(), Some("Apache-2.0"));
        assert!(!model.license.needs_acceptance);
    }
}

#[test]
fn the_embedder_says_how_it_embeds() {
    let catalog = catalog::bundled().unwrap();
    let embed = catalog.model("embeddinggemma-2-q8_0").unwrap();
    let e = embed.embedding.as_ref().unwrap();
    assert_eq!(e.dims, 768);
    assert!(e.mrl_dims.iter().all(|d| *d < e.dims));
    assert!(catalog
        .models
        .iter()
        .filter(|m| m.embedding.is_some())
        .all(|m| m.capabilities.contains(&Capability::Embed)));
}

/// Prompt formats = catalog data (engine crate rule): every named profile exists; engine floors per vendored build.
#[test]
fn every_model_names_prompt_data_the_catalog_holds() {
    let catalog = catalog::bundled().unwrap();
    for model in &catalog.models {
        if let Some(name) = &model.prompt_profile {
            let profile = catalog.prompt_profiles.get(name).unwrap();
            assert!(
                !profile.generation.is_empty() && !profile.stop.is_empty(),
                "{name}"
            );
        }
        if let Some(e) = &model.embedding {
            let prefixes = catalog.prefix_profiles.get(&e.prefix_profile).unwrap();
            assert!(!prefixes.query.is_empty() && !prefixes.document.is_empty());
        }
    }
    // model card (google/embeddinggemma-2 config_sentence_transformers.json)
    let gemma = &catalog.prefix_profiles["embeddinggemma-2"];
    assert_eq!(gemma.query, "task: search result | query: ");
    assert_eq!(gemma.document, "title: none | text: ");
    assert!(matches!(
        edited(|v| v["models"][1]["promptProfile"] = json!("nope")),
        Err(CatalogError::Profile(_))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["embedding"]["prefixProfile"] = json!("nope")),
        Err(CatalogError::Profile(_))
    ));
}

/// The bundled file with one edit (`edit` gets the JSON of the first model).
fn edited(edit: impl FnOnce(&mut Value)) -> Result<Catalog, CatalogError> {
    let mut value: Value = serde_json::from_str(include_str!("catalog.json")).unwrap();
    edit(&mut value);
    Catalog::parse(&value.to_string())
}

#[test]
fn validation_refuses_broken_entries() {
    let first = |v: &mut Value| v["models"][0].clone();
    assert!(matches!(
        edited(|v| v["schemaVersion"] = json!(2)),
        Err(CatalogError::Schema(2))
    ));
    assert!(matches!(
        edited(|v| {
            let copy = first(v);
            v["models"].as_array_mut().unwrap().push(copy);
        }),
        Err(CatalogError::DuplicateId(_))
    ));
    assert!(matches!(
        edited(|v| {
            v["models"][0]["publisher"] = json!("someone");
            v["models"][0]["repo"] = json!("someone/x");
        }),
        Err(CatalogError::Publisher(_))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["repo"] = json!("LiquidAI/x")),
        Err(CatalogError::Repo(_))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["commit"] = json!("main")),
        Err(CatalogError::Commit(_))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["files"][0]["bytes"] = json!(0)),
        Err(CatalogError::Pin(..))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["files"][0]["sha256"] = json!("ABC")),
        Err(CatalogError::Pin(..))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["files"][0]["path"] = json!("../x.gguf")),
        Err(CatalogError::Address(..))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["capabilities"] = json!(["embed", "chat"])),
        Err(CatalogError::Capabilities(_))
    ));
    assert!(matches!(
        edited(|v| v["models"][0]["extra"] = json!(1)),
        Err(CatalogError::Parse(_))
    ));
    // S5: terms to accept carry a revision
    for revision in [Value::Null, json!(" ")] {
        assert!(matches!(
            edited(|v| {
                v["models"][0]["license"]["needsAcceptance"] = json!(true);
                v["models"][0]["license"]["revision"] = revision;
            }),
            Err(CatalogError::License(_))
        ));
    }
}

/// A gated model needs nothing beyond `gated: true`.
#[test]
fn a_gated_model_is_just_marked() {
    let catalog = edited(|v| v["models"][0]["gated"] = json!(true)).unwrap();
    assert!(catalog.models[0].gated);
}

// ---- tasks (AC-8 registry)

#[derive(Deserialize)]
struct Fixture {
    tasks: Vec<TaskInfo>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    capabilities: Vec<Capability>,
    fits: Vec<Task>,
}

#[test]
fn tasks_match_the_shared_fixture() {
    let fixture: Fixture = serde_json::from_str(TASKS_FIXTURE).unwrap();
    assert_eq!(fixture.tasks, tasks::infos());
    assert!(fixture.cases.len() >= 8);
    for case in &fixture.cases {
        let fits: Vec<_> = Task::ALL
            .into_iter()
            .filter(|t| t.accepts(&case.capabilities))
            .collect();
        assert_eq!(fits, case.fits, "{}", case.name);
    }
}

#[test]
fn every_task_but_read_aloud_has_a_bundled_model() {
    let catalog = catalog::bundled().unwrap();
    for task in Task::ALL {
        let any = catalog.models.iter().any(|m| task.accepts(&m.capabilities));
        // no voice model in the initial catalog (OS voices first, epic D-6)
        assert_eq!(any, task != Task::ReadAloud, "{task:?}");
    }
}

/// AC-6: "Remove data" tells the engine files too (plugin off: read from the profile).
#[test]
fn data_bytes_count_the_engine_files() {
    let tmp = tempfile::tempdir().unwrap();
    let app = StateStore::app(&AppDirs::under(tmp.path()));
    let before = plugin().data_bytes(&app).unwrap();
    let unit = app
        .plugin_dir(&ID)
        .join(engine::files::DIR)
        .join("worker-v0.0.1-test-cpu");
    std::fs::create_dir_all(&unit).unwrap();
    std::fs::write(unit.join("worker.exe"), [0u8; 1234]).unwrap();
    assert_eq!(plugin().data_bytes(&app).unwrap(), before + 1234);
}
