// Test code: sizes here are small, and plain asserts read best.
#![allow(
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::format_push_string
)]

use std::collections::BTreeMap;
use std::sync::RwLock;

use permark_core::conditions::{Context, Distribution};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{CxWorkspace, RpcCall, RpcTable, Services, WindowFiles};
use permark_core::settings::AppSettings;
use serde_json::json;

use super::*;

fn estimate_counter(text: &str) -> (u64, Option<String>, bool) {
    (estimate(text), None, true)
}

fn run(text: &str, source: Source) -> Result<Done, Failure> {
    analyze(text, source, &Cancel::default(), &estimate_counter)
}

fn read_as(source: Source, text: &str) -> (Value, Vec<String>) {
    let cancel = Cancel::default();
    let mut losses = Losses::default();
    let value = read(source, text, &mut Tick::new(&cancel), &mut losses)
        .unwrap_or_else(|e| panic!("{source:?}: {e:?}\n{text}"));
    (value, losses.words())
}

fn write_as(target: Target, value: &Value) -> String {
    let cancel = Cancel::default();
    write(
        target,
        value,
        &mut Tick::new(&cancel),
        &mut Losses::default(),
    )
    .unwrap()
}

/// JSON-model equality: numbers by value.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((k1, v1), (k2, v2))| k1 == k2 && same(v1, v2))
        }
        _ => a == b,
    }
}

fn corpus() -> Vec<(&'static str, Value)> {
    vec![
        (
            "table",
            json!([
                { "id": 1, "name": "Ada", "role": "admin", "active": true },
                { "id": 2, "name": "Bob, Jr", "role": "user \"guest\"", "active": false },
                { "id": 3, "name": "Cy: x", "role": "", "active": true }
            ]),
        ),
        (
            "nested",
            json!({
                "server": { "host": "localhost", "port": 8080, "tls": { "on": true, "cert": "/etc/c.pem" } },
                "limits": [10, 20.5, -3e-7],
                "empty": {},
                "none": [],
                "nothing": null,
                "text": "line one\nline two\ttab",
                "unicode": "日本語 ünï 😀",
                "tricky": ["- dash", "# hash", "yes", "null", "123", " edge ", "a: b", "[x]", "{y}", "@z", "", "true"]
            }),
        ),
        (
            "mixed list",
            json!({ "items": [1, "two", { "three": 3 }, [4, 5], { "a": [ { "b": 1 }, { "c": 2 } ] }] }),
        ),
        (
            "records",
            json!({ "orders": [
                { "id": 1, "customer": { "name": "Ada", "country": "UK" }, "total": 9.5 },
                { "id": 2, "customer": { "name": "Bob", "country": "US" }, "total": 12 }
            ] }),
        ),
        ("scalar root", json!("just text")),
        ("number root", json!(42)),
    ]
}

#[test]
fn every_lossless_target_reads_back_to_the_same_data() {
    for (name, value) in corpus() {
        for (target, source) in [
            (Target::Toon, Source::Toon),
            (Target::Json, Source::Json),
            (Target::JsonPretty, Source::Json),
            (Target::Yaml, Source::Yaml),
        ] {
            let text = write_as(target, &value);
            let (back, _) = read_as(source, &text);
            assert!(
                same(&back, &value),
                "{name} via {target:?}:\n{text}\n-> {back}"
            );
        }
        if value.is_array() {
            let text = write_as(Target::JsonLines, &value);
            let (back, _) = read_as(Source::JsonLines, &text);
            assert!(same(&back, &value), "{name} via JSON Lines");
        }
    }
}

#[test]
fn csv_is_only_for_one_uniform_table_and_reads_back() {
    let corpus = corpus();
    let table = &corpus[0].1;
    for (target, source) in [(Target::Csv, Source::Csv), (Target::Tsv, Source::Tsv)] {
        let text = write_as(target, table);
        let (back, losses) = read_as(source, &text);
        assert!(same(&back, table), "{target:?}:\n{text}\n-> {back}");
        assert_eq!(losses, ["types"]);
    }
    let done = run(&serde_json::to_string(&corpus[1].1).unwrap(), Source::Json).unwrap();
    for id in ["csv", "tsv", "jsonl"] {
        let t = done.analysis.targets.iter().find(|t| t.id == id).unwrap();
        assert_eq!(t.fit, "no", "{id}");
        assert!(t.tokens.is_none());
    }
    // null cells say they became empty
    let mut losses = Losses::default();
    let cancel = Cancel::default();
    let text = write(
        Target::Csv,
        &json!([{ "a": null, "b": 1 }]),
        &mut Tick::new(&cancel),
        &mut losses,
    )
    .unwrap();
    assert_eq!(text, "a,b\n,1\n");
    assert_eq!(losses.words(), ["nulls"]);
}

#[test]
fn xml_maps_by_the_fixed_rules() {
    let xml = r#"<?xml version="1.0"?>
<!-- a comment -->
<catalog id="7" lang="en">
  <book id="b1"><title>Rust &amp; you</title><price>10</price></book>
  <book id="b2"><title><![CDATA[<raw>]]></title><price>12</price></book>
  <note>plain &#65;</note>
  <empty/>
  <?pi target?>
</catalog>"#;
    let (value, losses) = read_as(Source::Xml, xml);
    assert_eq!(
        value,
        json!({ "catalog": {
            "@id": "7", "@lang": "en",
            "book": [
                { "@id": "b1", "title": "Rust & you", "price": "10" },
                { "@id": "b2", "title": "<raw>", "price": "12" }
            ],
            "note": "plain A",
            "empty": ""
        } })
    );
    for word in ["comments", "instructions", "whitespace"] {
        assert!(losses.iter().any(|l| l == word), "{word} in {losses:?}");
    }
    let (mixed, losses) = read_as(Source::Xml, "<p>Hello <b>you</b> there</p>");
    assert_eq!(
        mixed,
        json!({ "p": { "b": "you", "#text": "Hello  there" } })
    );
    assert!(losses.iter().any(|l| l == "mixed"));
    let (_, losses) = read_as(Source::Xml, r#"<x:a xmlns:x="urn:x"><x:b>1</x:b></x:a>"#);
    assert!(losses.iter().any(|l| l == "namespaces"));
    let done = run("<p>Hello <b>you</b> there</p>", Source::Xml).unwrap();
    assert_eq!(
        done.analysis
            .targets
            .iter()
            .find(|t| t.id == "csv")
            .unwrap()
            .fit,
        "no"
    );
    for bad in ["<a><b></a>", "<a>", "text", "<a/><b/>"] {
        let mut losses = Losses::default();
        let cancel = Cancel::default();
        assert!(
            matches!(
                read(Source::Xml, bad, &mut Tick::new(&cancel), &mut losses),
                Err(Failure::Invalid { .. })
            ),
            "{bad}"
        );
    }
}

#[test]
fn yaml_reads_documents_anchors_and_refuses_bombs() {
    let (value, losses) = read_as(
        Source::Yaml,
        "# config\nbase: &b { a: 1 }\nuse: *b\nlist:\n  - x\n  - 2\n  - 'yes'\n  - true\n",
    );
    assert_eq!(
        value,
        json!({ "base": { "a": 1 }, "use": { "a": 1 }, "list": ["x", 2, "yes", true] })
    );
    assert!(losses.iter().any(|l| l == "comments") && losses.iter().any(|l| l == "anchors"));
    let (docs, losses) = read_as(Source::Yaml, "a: 1\n---\nb: 2\n");
    assert_eq!(docs, json!([{ "a": 1 }, { "b": 2 }]));
    assert!(losses.iter().any(|l| l == "documents"));
    let mut bomb = String::from("a: &a [x, x, x, x, x, x, x, x, x, x]\n");
    for n in 1..9 {
        bomb.push_str(&format!(
            "{}: &{} [{}]\n",
            (b'a' + n) as char,
            (b'a' + n) as char,
            vec![format!("*{}", (b'a' + n - 1) as char); 10].join(", ")
        ));
    }
    let mut losses = Losses::default();
    let cancel = Cancel::default();
    assert!(matches!(
        read(Source::Yaml, &bomb, &mut Tick::new(&cancel), &mut losses),
        Err(Failure::Invalid { .. })
    ));
}

#[test]
fn nesting_deeper_than_the_limit_is_refused_and_never_overflows_the_stack() {
    let deep = 5000;
    let xml = format!("{}{}", "<a>".repeat(deep), "</a>".repeat(deep));
    let block = format!(
        "{}x
",
        "- ".repeat(deep)
    );
    let flow = format!("{}{}", "[".repeat(deep), "]".repeat(deep));
    let maps = (0..deep).fold(String::new(), |mut text, i| {
        text.push_str(&" ".repeat(i));
        text.push_str(
            "k:
",
        );
        text
    });
    // each alias doubles nothing but adds a level: chained anchors cannot build past the limit
    let mut chain = String::from(
        "a0: &a0 [x]
",
    );
    for i in 1..400 {
        chain.push_str(&format!(
            "a{i}: &a{i} [*a{}]
",
            i - 1
        ));
    }
    for (source, text) in [
        (Source::Xml, xml),
        (Source::Yaml, block),
        (Source::Yaml, flow),
        (Source::Yaml, maps),
        (Source::Yaml, chain),
    ] {
        // the worker threads that run conversions have 2 MiB of stack
        let outcome = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || run(&text, source).err())
            .unwrap()
            .join()
            .unwrap();
        assert!(
            matches!(outcome, Some(Failure::Invalid { .. })),
            "{source:?}: {outcome:?}"
        );
    }
    // deep, but within the limit
    let (value, _) = read_as(
        Source::Yaml,
        &format!("{}x{}", "[".repeat(200), "]".repeat(200)),
    );
    assert!(value.is_array());
    let (value, _) = read_as(
        Source::Xml,
        &format!("{}{}", "<a>".repeat(200), "</a>".repeat(200)),
    );
    assert!(value.is_object());
}

#[test]
fn toml_and_toon_and_json_lines_read() {
    let (value, losses) = read_as(
        Source::Toml,
        "# c\n[package]\nname = \"x\"\nwhen = 2026-10-19\n[[bin]]\nname = \"a\"\n[[bin]]\nname = \"b\"\n",
    );
    assert_eq!(
        value,
        json!({ "package": { "name": "x", "when": "2026-10-19" }, "bin": [{ "name": "a" }, { "name": "b" }] })
    );
    assert_eq!(losses, ["comments", "dates"]);
    let (value, losses) = read_as(Source::Toon, "# c\nusers[2]{id,name}:\n  1,Ada\n  2,Bob");
    assert_eq!(
        value,
        json!({ "users": [{ "id": 1, "name": "Ada" }, { "id": 2, "name": "Bob" }] })
    );
    assert_eq!(losses, ["comments"]);
    let (value, _) = read_as(Source::JsonLines, "{\"a\":1}\n\n{\"a\":2}\n");
    assert_eq!(value, json!([{ "a": 1 }, { "a": 2 }]));
    let mut losses = Losses::default();
    let cancel = Cancel::default();
    let bad = read(
        Source::JsonLines,
        "{\"a\":1}\n{oops\n",
        &mut Tick::new(&cancel),
        &mut losses,
    );
    assert!(matches!(bad, Err(Failure::Invalid { line: 2, .. })));
}

#[test]
fn csv_reading_guesses_types_and_names_repeated_columns() {
    let (value, losses) = read_as(Source::Csv, "a,a,b\n1,2,x\n05,1.50,true\n3\n");
    assert_eq!(
        value,
        json!([
            { "a": 1, "a_2": 2, "b": "x" },
            { "a": "05", "a_2": "1.50", "b": true },
            { "a": 3, "a_2": "", "b": "" }
        ])
    );
    assert_eq!(losses, ["duplicateColumns", "uneven", "types"]);
}

#[test]
fn a_table_is_best_as_toon_or_csv_and_deep_data_is_said_to_gain_nothing() {
    let rows: Vec<Value> = (0..60)
        .map(|i| json!({ "id": i, "path": format!("docs/file-{i}.md"), "size": i * 37, "done": i % 2 == 0 }))
        .collect();
    let source = serde_json::to_string_pretty(&Value::Array(rows)).unwrap();
    let done = run(&source, Source::Json).unwrap();
    let a = &done.analysis;
    assert_eq!(a.from, "json");
    assert!(a.approx && a.tokenizer.is_none());
    let toon = a.targets.iter().find(|t| t.id == "toon").unwrap();
    assert_eq!(
        (toon.fit.as_str(), toon.reason.as_str()),
        ("good", "tabular")
    );
    let best = a.targets.iter().find(|t| t.best).unwrap();
    assert!(
        ["toon", "csv", "tsv"].contains(&best.id.as_str()),
        "{best:?}"
    );
    // the estimate of a target is the estimator of its text
    let toon_text = done.outputs.get(&Target::Toon).unwrap();
    assert_eq!(toon.tokens, Some(estimate(toon_text)));
    assert_eq!(a.source_tokens, estimate(&source));
    assert!(toon.preview.lines().count() <= PREVIEW_LINES + 1);

    // irregular, deep data counted by bytes: TOON is not smaller than compact JSON, and the dialog says so
    let deep = json!({ "a": { "b": { "c": { "d": [ { "x": 1 }, { "y": [1, { "z": 2 }] }, { "w": { "v": "u" } } ] } } } });
    let compact = serde_json::to_string(&deep).unwrap();
    let bytes = |t: &str| (t.len() as u64, None, true);
    let done = analyze(&compact, Source::Json, &Cancel::default(), &bytes).unwrap();
    let toon = done
        .analysis
        .targets
        .iter()
        .find(|t| t.id == "toon")
        .unwrap();
    assert_eq!(toon.fit, "poor", "{toon:?}");
    assert!(["notUniform", "deep", "noGain"].contains(&toon.reason.as_str()));
    let json = done
        .analysis
        .targets
        .iter()
        .find(|t| t.id == "json")
        .unwrap();
    assert_eq!(json.reason, "noGain");
    let pretty = done
        .analysis
        .targets
        .iter()
        .find(|t| t.id == "jsonPretty")
        .unwrap();
    assert_eq!(
        (pretty.fit.as_str(), pretty.reason.as_str()),
        ("poor", "readable")
    );
}

#[test]
fn secrets_are_counted_and_masked_in_previews_only() {
    let data = json!({ "db": { "host": "h", "password": "hunter2" }, "url": "postgres://u:pw1@h/db", "api_key": "${KEY}", "token": 1234 });
    assert_eq!(count_secrets(&data), 3);
    let masked = mask_secrets(&data);
    let text = masked.to_string();
    assert!(!text.contains("hunter2") && !text.contains("pw1") && !text.contains("1234"));
    assert!(text.contains("${KEY}"));
    let done = run(&data.to_string(), Source::Json).unwrap();
    assert_eq!(done.analysis.secrets, 3);
    for t in &done.analysis.targets {
        assert!(!t.preview.contains("hunter2"), "{}", t.id);
    }
    assert!(done.outputs.get(&Target::Toon).unwrap().contains("hunter2"));
}

#[test]
fn too_large_and_cancelled_work_stop() {
    let big = "x".repeat(MAX_SOURCE as usize + 1);
    assert_eq!(run(&big, Source::Json).err(), Some(Failure::TooLarge));
    let cancel = Cancel::default();
    cancel.cancel();
    let rows: Vec<Value> = (0..20_000).map(|i| json!({ "i": i })).collect();
    let text = Value::Array(rows).to_string();
    assert_eq!(
        analyze(&text, Source::Json, &cancel, &estimate_counter).err(),
        Some(Failure::Cancelled)
    );
}

#[test]
fn sources_are_sniffed() {
    for (text, source) in [
        ("{\"a\": 1}", Source::Json),
        ("{\"a\":1}\n{\"a\":2}", Source::JsonLines),
        ("<a/>", Source::Xml),
        ("users[2]{id,name}:\n  1,a\n  2,b", Source::Toon),
        ("a\tb\n1\t2", Source::Tsv),
        ("a,b\n1,2", Source::Csv),
        ("[package]\nname = \"x\"", Source::Toml),
        ("a: 1\nb:\n  - x", Source::Yaml),
    ] {
        assert_eq!(Source::sniff(text), source, "{text}");
    }
    assert_eq!(Source::parse("YML"), Some(Source::Yaml));
    assert_eq!(Target::parse("jsonPretty"), Some(Target::JsonPretty));
}

// ---- the plugin through its RPC -------------------------------------------------------------------------------------

struct Quiet;
impl EventSink for Quiet {
    fn emit(&self, _: CoreEvent) {}
}

/// Files of a pretend workspace: a map of paths, never overwritten.
#[derive(Default)]
struct FakeFiles(
    Mutex<BTreeMap<String, String>>,
    Mutex<Vec<permark_core::activity::Fact>>,
);

impl WindowFiles for FakeFiles {
    fn note(&self, _window: &str, fact: permark_core::activity::Fact) {
        self.1.lock().unwrap().push(fact);
    }

    fn read(&self, _window: &str, path: &str, cap: u64) -> Result<String, IpcError> {
        let files = self.0.lock().unwrap();
        let text = files
            .get(path)
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound))?;
        if text.len() as u64 > cap {
            return Err(IpcError::new(ErrorKind::TooLarge));
        }
        Ok(text.clone())
    }

    fn write_beside(
        &self,
        _window: &str,
        beside: &str,
        name: &str,
        text: &str,
    ) -> Result<String, IpcError> {
        let mut files = self.0.lock().unwrap();
        let dir = beside
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| format!("{d}/"));
        let (stem, ext) = name.rsplit_once('.').unwrap();
        for n in 1..100 {
            let candidate = if n == 1 {
                format!("{dir}{name}")
            } else {
                format!("{dir}{stem} ({n}).{ext}")
            };
            if !files.contains_key(&candidate) {
                files.insert(candidate.clone(), text.to_owned());
                return Ok(candidate);
            }
        }
        Err(IpcError::new(ErrorKind::Exists))
    }
}

struct Harness {
    rpc: Arc<RpcTable>,
    files: Arc<FakeFiles>,
    _lease: permark_core::plugins::ServiceLease,
    _running: Box<dyn Running>,
}

impl Harness {
    fn start() -> Self {
        let rpc = RpcTable::new();
        let services = Services::new();
        let files = Arc::new(FakeFiles::default());
        let lease = services.provide(Arc::new(WindowFilesApi(
            Arc::clone(&files) as Arc<dyn WindowFiles>
        )));
        let cx = PluginCx::new(
            ID,
            CxWorkspace::App,
            std::env::temp_dir(),
            None,
            Context::welcome(Distribution::Direct),
            Arc::new(Quiet),
        )
        .with_app(AppServices::new(
            Arc::clone(&rpc),
            services,
            Arc::new(RwLock::new(AppSettings::default())),
        ));
        let running = plugin().start(&cx).unwrap();
        Self {
            rpc,
            files,
            _lease: lease,
            _running: running,
        }
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, IpcError> {
        self.rpc.call(
            "convert",
            method,
            &RpcCall {
                window: "main".into(),
                args,
            },
        )
    }
}

#[test]
fn a_file_converts_and_saves_next_to_itself_never_over_a_file() {
    let h = Harness::start();
    h.files.0.lock().unwrap().insert(
        "data/users.json".into(),
        r#"[{"id":1,"name":"Ada"},{"id":2,"name":"Bob"}]"#.into(),
    );
    h.files
        .0
        .lock()
        .unwrap()
        .insert("data/users.toon".into(), "taken".into());
    let answer = h
        .call(
            "analyze",
            json!({ "job": 1, "source": { "kind": "file", "path": "data/users.json" } }),
        )
        .unwrap();
    assert_eq!(answer["status"], "ready");
    assert_eq!(answer["analysis"]["from"], "json");
    let result = h
        .call("result", json!({ "job": 1, "target": "toon" }))
        .unwrap();
    assert_eq!(result, "[2]{id,name}:\n  1,Ada\n  2,Bob");
    let saved = h
        .call("save", json!({ "job": 1, "target": "toon" }))
        .unwrap();
    assert_eq!(saved, "data/users (2).toon");
    assert_eq!(h.files.0.lock().unwrap()["data/users.toon"], "taken");
    assert_eq!(
        h.files.0.lock().unwrap()["data/users.json"],
        r#"[{"id":1,"name":"Ada"},{"id":2,"name":"Bob"}]"#
    );
    // a text from the page saves next to the document it came from
    h.call(
        "analyze",
        json!({ "job": 2, "source": { "kind": "text", "text": "a: 1" }, "from": "yaml" }),
    )
    .unwrap();
    assert!(h
        .call("save", json!({ "job": 2, "target": "json" }))
        .is_err());
    let saved = h
        .call(
            "save",
            json!({ "job": 2, "target": "json", "beside": "notes/today.md" }),
        )
        .unwrap();
    assert_eq!(saved, "notes/today.json");
    // the journal heard the copy and both saves, with what was converted and where it went
    let noted = h.files.1.lock().unwrap().clone();
    let convert =
        |path: &str, to: &str, saved: Option<&str>| permark_core::activity::Fact::Convert {
            path: path.to_owned(),
            to: to.to_owned(),
            saved: saved.map(str::to_owned),
        };
    assert_eq!(
        noted,
        [
            convert("data/users.json", "toon", None),
            convert("data/users.json", "toon", Some("data/users (2).toon")),
            convert("notes/today.md", "json", Some("notes/today.json")),
        ]
    );
    // the earlier job of the window is gone
    assert!(h
        .call("result", json!({ "job": 1, "target": "toon" }))
        .is_err());
    h.call("close", json!({ "job": 2 })).unwrap();
    assert!(h
        .call("result", json!({ "job": 2, "target": "json" }))
        .is_err());
}

#[test]
fn secret_values_leave_only_after_confirmation() {
    let h = Harness::start();
    h.call(
        "analyze",
        json!({ "job": 1, "source": { "kind": "text", "text": "{\"password\":\"hunter2\"}" } }),
    )
    .unwrap();
    let refused = h
        .call("result", json!({ "job": 1, "target": "toon" }))
        .unwrap_err();
    assert_eq!(refused.detail.as_deref(), Some("secrets"));
    assert!(h
        .call(
            "save",
            json!({ "job": 1, "target": "toon", "beside": "a.md" })
        )
        .is_err());
    let text = h
        .call(
            "result",
            json!({ "job": 1, "target": "toon", "confirmed": true }),
        )
        .unwrap();
    assert_eq!(text, "password: hunter2");
}

#[test]
fn refusals_come_back_as_answers() {
    let h = Harness::start();
    let big = "x".repeat(MAX_SOURCE as usize + 10);
    h.files.0.lock().unwrap().insert("big.json".into(), big);
    let answer = h
        .call(
            "analyze",
            json!({ "job": 1, "source": { "kind": "file", "path": "big.json" } }),
        )
        .unwrap();
    assert_eq!(
        answer,
        json!({ "status": "refused", "reason": "tooLarge", "line": 0, "detail": "" })
    );
    let answer = h
        .call(
            "analyze",
            json!({ "job": 2, "source": { "kind": "text", "text": "{\n\"a\": oops}" }, "from": "json" }),
        )
        .unwrap();
    assert_eq!(answer["status"], "refused");
    assert_eq!(answer["reason"], "invalid");
    assert_eq!(answer["line"], 2);
    h.call("cancel", json!({ "job": 99 })).unwrap();
}

#[test]
fn the_plugin_is_off_by_default_and_needs_the_toon_format() {
    let p = plugin();
    let m = p.manifest();
    assert_eq!(m.id.as_str(), "convert");
    assert!(!m.default_enabled);
    assert_eq!(m.kind, PluginKind::Feature);
    assert_eq!(m.depends_on.as_ref(), [FMT_TOON]);
}
