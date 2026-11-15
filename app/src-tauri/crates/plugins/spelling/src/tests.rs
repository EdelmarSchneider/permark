// Test code: plain `assert!(x.is_empty())` reads best.
#![allow(clippy::assert_is_empty, clippy::items_after_statements)]

use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use permark_core::conditions::{Context, Distribution};
use permark_core::downloads::{Asset, MemorySecrets, Scripted, ScriptedTransport, Transport};
use permark_core::events::{CoreEvent, EventSink};
use permark_core::plugins::{AppServices, CxWorkspace, RpcCall, RpcTable, ServiceLease, Services};
use permark_core::settings::AppSettings;
use permark_core::state::{AppDirs, StateStore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;

// ---- pure rules ----

#[test]
fn normalize_composes_and_maps_apostrophes() {
    assert!(matches!(normalize("hello", "en"), Cow::Borrowed("hello")));
    assert_eq!(normalize("cafe\u{301}", "fr"), "café");
    assert_eq!(normalize("l\u{2019}acqua", "it"), "l'acqua");
}

#[test]
fn normalize_gives_romanian_its_comma_letters_only() {
    assert_eq!(normalize("ştiinţă", "ro"), "știință");
    assert_eq!(normalize("ŞŢ", "ro-RO"), "ȘȚ");
    // decomposed cedilla: composed first, then mapped
    assert_eq!(normalize("s\u{327}", "ro"), "ș");
    // other languages keep the cedilla
    assert!(matches!(normalize("ş", "en"), Cow::Borrowed("ş")));
}

#[test]
fn flag_kind_is_read_from_the_aff() {
    assert_eq!(FlagKind::of("SET UTF-8\n"), FlagKind::Short);
    assert_eq!(FlagKind::of("SET UTF-8\nFLAG num\n"), FlagKind::Numeric);
    assert_eq!(FlagKind::of("FLAG UTF-8\n"), FlagKind::Utf8);
    assert_eq!(
        FlagKind::of("FLAG long\nAF 1\nAF AbCd\n"),
        FlagKind::Long { aliased: true }
    );
    assert_eq!(
        FlagKind::of("FLAG long\n"),
        FlagKind::Long { aliased: false }
    );
}

#[test]
fn sanitize_drops_only_flags_the_engine_cannot_read() {
    let dic = "5\ngood/1,2\nbad/1,2x3,4 po:verb\nplain po:noun [n: a/b]\nA\\/B po:abbr\nlast/7\n";
    let fixed = sanitize_dic(dic, FlagKind::Numeric);
    assert_eq!(
        fixed,
        "5\ngood/1,2\nbad\nplain po:noun [n: a/b]\nA\\/B po:abbr\nlast/7\n"
    );
    // a clean dictionary is not copied
    let clean = "2\ngood/1,2\nother\n";
    assert!(matches!(
        sanitize_dic(clean, FlagKind::Numeric),
        Cow::Borrowed(_)
    ));
    assert!(matches!(
        sanitize_dic(dic, FlagKind::Short),
        Cow::Borrowed(_)
    ));
    // zero, empty part, odd long flags
    assert_eq!(
        sanitize_dic("3\na/0\nb/1,,2\nc/12", FlagKind::Numeric),
        "3\na\nb\nc/12"
    );
    assert_eq!(
        sanitize_dic("2\nab/AbC\ncd/AbCd\n", FlagKind::Long { aliased: false }),
        "2\nab\ncd/AbCd\n"
    );
    assert_eq!(
        sanitize_dic("1\nab/123\r\n", FlagKind::Long { aliased: true }),
        "1\nab/123\r\n"
    );
}

#[test]
fn a_sanitized_dictionary_loads() {
    let aff = "SET UTF-8\nFLAG num\nSFX 10 Y 1\nSFX 10 0 s .\n";
    let dic = "2\ncat/10\ndog/10,2iñer30 po:noun\n";
    assert!(Dictionary::new(aff, dic).is_err());
    let fixed = sanitize_dic(dic, FlagKind::of(aff));
    let dictionary = Dictionary::new(aff, &fixed).unwrap();
    assert!(dictionary.check("cats"));
    assert!(dictionary.check("dog"));
    assert!(!dictionary.check("dogs"));
}

#[test]
fn spaced_suggestions_need_every_part_to_be_a_word() {
    let known = |w: &str| ["usa", "de", "a"].contains(&w);
    assert!(keep_suggestion("usada", known));
    assert!(keep_suggestion("usa de", known));
    assert!(!keep_suggestion("usa d", known));
}

#[test]
fn personal_words_accept_their_capitalized_and_upper_forms() {
    let words: BTreeSet<String> = ["permark", "iPhone", "it's"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    for ok in [
        "permark",
        "Permark",
        "PERMARK",
        "iPhone",
        "it\u{2019}s",
        "It's",
    ] {
        assert!(personal_accepts(&words, ok), "{ok}");
    }
    for no in ["PerMark", "pERMARK", "iphone", "Iphone", "other"] {
        assert!(!personal_accepts(&words, no), "{no}");
    }
    assert_eq!(personal_form("  caf\u{e9} "), Some("café".to_owned()));
    assert_eq!(personal_form("two words"), None);
    assert_eq!(personal_form(""), None);
    assert_eq!(personal_form(&"a".repeat(MAX_WORD_CHARS + 1)), None);
}

#[test]
fn the_official_table_builds_a_unit_per_language() {
    for language in OFFICIAL {
        let unit = language.unit().unwrap();
        assert_eq!(unit.id(), language.id);
        let urls: Vec<String> = unit.assets().iter().map(Asset::url).collect();
        assert_eq!(
            urls[0],
            format!(
                "https://raw.githubusercontent.com/wooorm/dictionaries/{COMMIT}/dictionaries/{}/index.aff",
                language.folder
            )
        );
        assert!(!language.license.is_empty());
    }
    assert_eq!(
        OFFICIAL.iter().map(|l| l.id).collect::<Vec<_>>(),
        ["en", "en-GB", "pt-BR", "pt-PT", "es", "fr", "de", "it", "ca", "gl", "ro"]
    );
}

#[test]
fn the_manifest_is_an_app_feature_that_starts_on() {
    let manifest = plugin().manifest().clone();
    assert_eq!(manifest.id.as_str(), "spelling");
    assert_eq!(manifest.kind, PluginKind::Feature);
    assert_eq!(manifest.scope, PluginScope::App);
    assert!(manifest.default_enabled);
    assert!(manifest.depends_on.is_empty());
}

// ---- the plugin over a scripted network ----

const EN_AFF: &str =
    "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz'\nREP 1\nREP teh the\nSFX S Y 1\nSFX S 0 s .\n";
const EN_DIC: &str = "4\nhello/S\nworld/S\nthe\nit's\n";
const RO_AFF: &str = "SET UTF-8\nTRY aeiouștrnșc\n";
const RO_DIC: &str = "1\nștiință\n";

fn pin_of(body: &str) -> Pin {
    Pin {
        bytes: body.len() as u64,
        sha256: Sha256::digest(body.as_bytes()).into(),
    }
}

fn language(id: &'static str, aff: &str, dic: &str) -> Language {
    Language {
        id,
        folder: id,
        license: "MIT",
        aff: pin_of(aff),
        dic: pin_of(dic),
    }
}

fn url_of(folder: &str, file: &str) -> String {
    format!("https://raw.githubusercontent.com/wooorm/dictionaries/{COMMIT}/dictionaries/{folder}/{file}")
}

#[derive(Default)]
struct Events(Mutex<Vec<(String, Value)>>);

impl EventSink for Events {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            self.0.lock().unwrap().push((name, payload));
        }
    }
}

struct Harness {
    tmp: tempfile::TempDir,
    dirs: AppDirs,
    rpc: Arc<RpcTable>,
    services: Arc<Services>,
    settings: Arc<RwLock<AppSettings>>,
    events: Arc<Events>,
    source: Arc<ScriptedTransport>,
    _kit: ServiceLease,
    languages: Vec<Language>,
    running: Option<Box<dyn Running>>,
    /// The window files service, once a test offers one.
    files: Option<ServiceLease>,
}

/// The network serves the fixture files of `en` and `ro` (`ro`'s dictionary as `ro_dic_served`).
fn network(ro_dic_served: &str) -> Arc<ScriptedTransport> {
    Arc::new(
        ScriptedTransport::new()
            .answer(&url_of("en", "index.aff"), Scripted::ok(EN_AFF.as_bytes()))
            .answer(&url_of("en", "index.dic"), Scripted::ok(EN_DIC.as_bytes()))
            .answer(&url_of("ro", "index.aff"), Scripted::ok(RO_AFF.as_bytes()))
            .answer(
                &url_of("ro", "index.dic"),
                Scripted::ok(ro_dic_served.as_bytes()),
            ),
    )
}

impl Harness {
    fn start() -> Self {
        Self::over(network(RO_DIC))
    }

    fn over(source: Arc<ScriptedTransport>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(tmp.path());
        let rpc = RpcTable::new();
        let services = Services::new();
        let kit = services.provide(Arc::new(DownloadKit {
            transport: Arc::clone(&source) as Arc<dyn Transport>,
            secrets: Arc::new(MemorySecrets::default()),
        }));
        let mut h = Self {
            tmp,
            dirs,
            rpc,
            services,
            settings: Arc::new(RwLock::new(AppSettings::default())),
            events: Arc::new(Events::default()),
            source,
            _kit: kit,
            languages: vec![
                language("en", EN_AFF, EN_DIC),
                language("ro", RO_AFF, RO_DIC),
            ],
            running: None,
            files: None,
        };
        h.restart();
        h
    }

    fn restart(&mut self) {
        self.running = None;
        let store = StateStore::app(&self.dirs);
        let cx = PluginCx::new(
            ID,
            CxWorkspace::App,
            store.plugin_dir(&ID),
            None,
            Context::welcome(Distribution::Direct),
            Arc::clone(&self.events) as Arc<dyn EventSink>,
        )
        .with_store(store)
        .with_app(AppServices::new(
            Arc::clone(&self.rpc),
            Arc::clone(&self.services),
            Arc::clone(&self.settings),
        ));
        self.running = Some(plugin_with(self.languages.clone()).start(&cx).unwrap());
    }

    fn call(&self, method: &str, args: Value) -> Result<Value, IpcError> {
        self.call_in("main", method, args)
    }

    fn call_in(&self, window: &str, method: &str, args: Value) -> Result<Value, IpcError> {
        self.rpc.call(
            "spelling",
            method,
            &RpcCall {
                window: window.into(),
                args,
            },
        )
    }

    fn state_of(&self, lang: &str) -> Value {
        let list = self.call("languages", json!({})).unwrap();
        list.as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == lang)
            .unwrap()["state"]
            .clone()
    }

    fn wait(&self, lang: &str, kind: &str) {
        let start = Instant::now();
        loop {
            let state = self.state_of(lang);
            if state["kind"] == kind {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "stuck: {state}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn ready(&self, lang: &str) {
        self.call("download", json!({ "lang": lang })).unwrap();
        self.wait(lang, "ready");
    }

    fn check(&self, langs: &[&str], words: &[&str]) -> Result<Vec<bool>, IpcError> {
        self.call("check", json!({ "langs": langs, "words": words }))
            .map(|v| serde_json::from_value(v).unwrap())
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|e| {
            let path = e.path();
            if path.is_dir() {
                files_under(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

/// On by default: starting, listing, checking (refused) and stopping ask the network for nothing and write no file.
#[test]
fn starting_fetches_nothing_and_writes_nothing() {
    let mut h = Harness::start();
    let list = h.call("languages", json!({})).unwrap();
    assert_eq!(list[0]["id"], "en");
    assert_eq!(list[0]["state"]["kind"], "absent");
    assert_eq!(list[0]["license"], "MIT");
    assert_eq!(list[0]["bytes"], (EN_AFF.len() + EN_DIC.len()) as u64);
    let refused = h.check(&["en"], &["hello"]).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Unavailable);
    assert_eq!(refused.detail.as_deref(), Some("notReady"));
    assert!(h
        .call("suggest", json!({ "langs": ["en"], "word": "helo" }))
        .is_err());
    h.restart();
    h.running = None;
    assert!(h.source.asked().is_empty());
    assert_eq!(files_under(&h.dirs.profile), Vec::<PathBuf>::new());
    assert_eq!(files_under(&h.dirs.cache), Vec::<PathBuf>::new());
}

#[test]
fn a_downloaded_dictionary_checks_words() {
    let h = Harness::start();
    h.ready("en");
    assert_eq!(h.source.asked().len(), 2);
    let verdicts = h
        .check(
            &["en"],
            &["hello", "worlds", "it\u{2019}s", "helo", "Hello"],
        )
        .unwrap();
    assert_eq!(verdicts, [true, true, true, false, true]);
    // the files are in the app cache, in the plugin's folder
    let dir = h.dirs.cache.join("plugins").join("spelling").join("en");
    assert!(dir.join("index.aff").is_file());
    assert!(dir.join("index.dic").is_file());
    // the windows heard the download
    let events = h.events.0.lock().unwrap().clone();
    assert!(events.iter().all(|(name, _)| name == "spelling:download"));
    assert!(events
        .iter()
        .any(|(_, p)| p["lang"] == "en" && p["state"]["kind"] == "downloading"));
    assert_eq!(events.last().unwrap().1["state"]["kind"], "ready");
}

#[test]
fn a_word_is_accepted_when_any_ready_language_knows_it() {
    let h = Harness::start();
    h.ready("en");
    // ro not downloaded: ignored, never fetched
    assert_eq!(
        h.check(&["en", "ro"], &["hello", "ştiinţă"]).unwrap(),
        [true, false]
    );
    assert_eq!(h.source.asked().len(), 2);
    h.ready("ro");
    // Romanian cedilla letters read as the comma letters of its dictionary
    assert_eq!(
        h.check(&["en", "ro"], &["hello", "ştiinţă", "nimic"])
            .unwrap(),
        [true, true, false]
    );
}

#[test]
fn a_dictionary_is_ready_again_after_a_restart_without_a_download() {
    let mut h = Harness::start();
    h.ready("en");
    let asked = h.source.asked().len();
    h.restart();
    assert_eq!(h.state_of("en")["kind"], "ready");
    assert_eq!(h.check(&["en"], &["world"]).unwrap(), [true]);
    assert_eq!(h.source.asked().len(), asked);
}

#[test]
fn a_file_that_is_not_the_pinned_one_fails_the_download() {
    let h = Harness::over(network("1\nstiinta\n"));
    h.call("download", json!({ "lang": "ro" })).unwrap();
    h.wait("ro", "failed");
    assert_eq!(h.state_of("ro")["reason"]["kind"], "hash");
    assert!(h.check(&["ro"], &["știință"]).is_err());
}

#[test]
fn suggestions_come_from_each_language_in_order() {
    let h = Harness::start();
    h.ready("en");
    let out = h
        .call("suggest", json!({ "langs": ["en"], "word": "helo" }))
        .unwrap();
    let out: Vec<String> = serde_json::from_value(out).unwrap();
    assert!(out.contains(&"hello".to_owned()), "{out:?}");
    assert!(out.len() <= SUGGESTIONS_PER_LANGUAGE);
    let out = h
        .call("suggest", json!({ "langs": ["en", "en"], "word": "teh" }))
        .unwrap();
    let out: Vec<String> = serde_json::from_value(out).unwrap();
    assert_eq!(out.first().map(String::as_str), Some("the"));
    assert_eq!(
        out.iter().filter(|s| *s == "the").count(),
        1,
        "each suggestion once"
    );
}

#[test]
fn calls_are_refused_over_the_word_cap_and_for_unknown_languages() {
    let h = Harness::start();
    h.ready("en");
    let words: Vec<String> = (0..=MAX_WORDS).map(|n| format!("w{n}")).collect();
    let refused = h
        .call("check", json!({ "langs": ["en"], "words": words }))
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::TooLarge);
    let refused = h.check(&["xx"], &["hello"]).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::InvalidData);
    assert_eq!(refused.detail.as_deref(), Some("lang"));
    let refused = h.call("download", json!({ "lang": "ja" })).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::InvalidData);
}

#[test]
fn personal_words_are_accepted_everywhere_and_kept_in_the_profile() {
    let mut h = Harness::start();
    h.ready("en");
    assert_eq!(
        h.check(&["en"], &["permark", "Permark"]).unwrap(),
        [false, false]
    );
    let list = h
        .call("personal.add", json!({ "word": "permark" }))
        .unwrap();
    assert_eq!(list, json!(["permark"]));
    assert_eq!(
        h.check(&["en"], &["permark", "Permark", "PERMARK", "PerMark"])
            .unwrap(),
        [true, true, true, false]
    );
    let events = h.events.0.lock().unwrap().clone();
    assert_eq!(
        events.last().unwrap(),
        &("spelling:personal".to_owned(), json!(["permark"]))
    );
    // kept across a restart (profile file of the plugin)
    h.restart();
    assert_eq!(
        h.call("personal.list", json!({})).unwrap(),
        json!(["permark"])
    );
    let refused = h
        .call("personal.add", json!({ "word": "two words" }))
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::InvalidData);
    let list = h
        .call("personal.remove", json!({ "word": "permark" }))
        .unwrap();
    assert_eq!(list, json!([]));
    assert_eq!(h.check(&["en"], &["permark"]).unwrap(), [false]);
}

/// The real dictionaries: every language loads (Galician only after its `.dic` is sanitized) and knows a common word.
/// Needs the files: `SPELLING_DICTS=<folder with one subfolder per source folder> cargo test -- --ignored`.
#[test]
#[ignore = "needs the downloaded dictionaries"]
fn the_official_dictionaries_load() {
    let Ok(root) = std::env::var("SPELLING_DICTS") else {
        return;
    };
    let words = [
        ("en", "house"),
        ("en-GB", "colour"),
        ("pt-BR", "casa"),
        ("pt-PT", "casa"),
        ("es", "casa"),
        ("fr", "maison"),
        ("de", "Haus"),
        ("it", "casa"),
        ("ca", "casa"),
        ("gl", "casa"),
        ("ro", "știință"),
    ];
    for language in OFFICIAL {
        let dir = Path::new(&root).join(language.folder);
        let aff = std::fs::read_to_string(dir.join("index.aff")).unwrap();
        let dic = std::fs::read_to_string(dir.join("index.dic")).unwrap();
        assert_eq!(pin_of(&aff), language.aff, "{}", language.id);
        assert_eq!(pin_of(&dic), language.dic, "{}", language.id);
        let fixed = sanitize_dic(&dic, FlagKind::of(&aff));
        let dictionary =
            Dictionary::new(&aff, &fixed).unwrap_or_else(|e| panic!("{}: {e}", language.id));
        let word = words.iter().find(|(id, _)| *id == language.id).unwrap().1;
        assert!(
            dictionary.check(&normalize(word, language.id)),
            "{}",
            language.id
        );
    }
}

#[path = "docs_tests.rs"]
mod docs;
