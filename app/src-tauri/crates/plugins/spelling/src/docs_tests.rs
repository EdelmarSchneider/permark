//! The language a document is checked in: declared (project `.permark/` | profile), detected, default.

use permark_core::paths::{Root, VaultPath};
use permark_core::plugins::{WindowFiles, WindowFilesApi};
use permark_core::state::{init_project, RawFile};

use super::*;

const ENGLISH: &str = "I forgot my umbrella at home again, so I got soaked on the way to work. The bakery on the \
     corner sells the best bread in town, but it closes early. Could you call me back when you have a minute? It is \
     about the meeting tomorrow.";

/// Window `proj` shows a project (with `.permark/`), window `notes` the Notes; any other window is unknown.
struct Fake {
    project: PathBuf,
    store: StateStore,
    notes: PathBuf,
}

impl WindowFiles for Fake {
    fn read(&self, _: &str, _: &str, _: u64) -> Result<String, IpcError> {
        Err(IpcError::new(ErrorKind::Unsupported))
    }

    fn locate(&self, window: &str, path: &str) -> Result<PathBuf, IpcError> {
        let base = match window {
            "proj" => &self.project,
            "notes" => &self.notes,
            _ => return Err(IpcError::new(ErrorKind::NotFound).subject("window")),
        };
        let abs = base.join(VaultPath::parse(path)?.as_str());
        if abs.is_file() {
            Ok(abs)
        } else {
            Err(IpcError::new(ErrorKind::NotFound).subject(path))
        }
    }

    fn write_beside(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, IpcError> {
        Err(IpcError::new(ErrorKind::Unsupported))
    }

    fn note(&self, _: &str, _: permark_core::activity::Fact) {}

    fn shared_file(&self, window: &str, name: &str) -> Option<RawFile> {
        (window == "proj").then(|| self.store.shared_file(name))
    }
}

struct Places {
    project: PathBuf,
    notes: PathBuf,
}

impl Places {
    fn shared(&self) -> PathBuf {
        self.project.join(".permark").join("spelling.json")
    }
}

impl Harness {
    /// Offers the window files: a project with `docs/a.md` + `docs/b.md`, the Notes with `n.md`.
    fn offer_files(&mut self) -> Places {
        let project = self.tmp.path().join("proj");
        let notes = self.tmp.path().join("notes");
        std::fs::create_dir_all(project.join("docs")).unwrap();
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(project.join("docs").join("a.md"), "a").unwrap();
        std::fs::write(project.join("docs").join("b.md"), "b").unwrap();
        std::fs::write(notes.join("n.md"), "n").unwrap();
        let store = init_project(&self.dirs, &Root::open(&project).unwrap()).unwrap();
        self.files = Some(
            self.services
                .provide(Arc::new(WindowFilesApi(Arc::new(Fake {
                    project: project.clone(),
                    store,
                    notes: notes.clone(),
                })))),
        );
        Places { project, notes }
    }

    fn language(&self, window: &str, path: &str, sample: &str, ui: &str) -> Value {
        self.call_in(
            window,
            "language",
            json!({ "path": path, "sample": sample, "uiLocale": ui }),
        )
        .unwrap()
    }

    fn declare(&self, window: &str, path: &str, lang: Option<&str>) -> Result<Value, IpcError> {
        self.call_in(
            window,
            "declare",
            json!({ "path": path, "lang": lang, "sample": ENGLISH, "uiLocale": "en" }),
        )
    }
}

fn profile_file(h: &Harness) -> PathBuf {
    h.dirs
        .profile
        .join("plugins")
        .join("spelling")
        .join("languages.json")
}

#[test]
fn text_long_enough_is_detected_else_the_default_applies() {
    let h = Harness::start();
    let found = h.language("main", "a.md", ENGLISH, "ro");
    assert_eq!(
        found,
        json!({ "langs": ["en"], "source": "detected", "detected": "en" })
    );
    // too short to believe: the window's language is the first default
    let short = h.language("main", "a.md", "Hello there", "ro");
    assert_eq!(short, json!({ "langs": ["ro"], "source": "default" }));
    let unsupported = h.language(
        "main",
        "a.md",
        "오늘은 날씨가 정말 좋네요. 산책하러 갈까요?",
        "en",
    );
    assert_eq!(
        unsupported,
        json!({ "langs": [], "source": "unsupported", "script": "hangul" })
    );
    // nothing of this is written
    assert_eq!(files_under(&h.dirs.profile), Vec::<PathBuf>::new());
}

#[test]
fn the_default_follows_the_window_until_the_person_picks_one() {
    let mut h = Harness::start();
    let get = |h: &Harness, ui: &str| h.call("defaults.get", json!({ "uiLocale": ui })).unwrap();
    assert_eq!(get(&h, "ro"), json!({ "lang": "ro", "chosen": false }));
    // no dictionary for the window's language: English
    assert_eq!(get(&h, "ja"), json!({ "lang": "en", "chosen": false }));
    assert_eq!(files_under(&h.dirs.profile), Vec::<PathBuf>::new());
    let set = h.call("defaults.set", json!({ "lang": "ro" })).unwrap();
    assert_eq!(set, json!({ "lang": "ro", "chosen": true }));
    h.restart();
    assert_eq!(get(&h, "en"), json!({ "lang": "ro", "chosen": true }));
    assert_eq!(
        h.language("main", "a.md", "Hello", "en"),
        json!({ "langs": ["ro"], "source": "default" })
    );
    let refused = h
        .call("defaults.set", json!({ "lang": "pt-BR" }))
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::InvalidData);
    assert_eq!(refused.detail.as_deref(), Some("lang"));
}

#[test]
fn a_declared_language_of_a_project_document_travels_with_the_project() {
    let mut h = Harness::start();
    let places = h.offer_files();
    let answer = h.declare("proj", "docs/a.md", Some("ro")).unwrap();
    // declared wins over the English text
    assert_eq!(answer, json!({ "langs": ["ro"], "source": "declared" }));
    assert_eq!(
        h.language("proj", "docs\\a.md", ENGLISH, "en"),
        json!({ "langs": ["ro"], "source": "declared" })
    );
    // another document of the project is still detected
    assert_eq!(
        h.language("proj", "docs/b.md", ENGLISH, "en")["source"],
        "detected"
    );
    let shared: Value = serde_json::from_slice(&std::fs::read(places.shared()).unwrap()).unwrap();
    assert_eq!(
        shared,
        json!({ "version": 1, "data": { "docs": { "docs/a.md": "ro" } } })
    );
    // the profile holds none of it
    assert!(!profile_file(&h).exists());
    h.restart();
    assert_eq!(
        h.language("proj", "docs/a.md", ENGLISH, "en")["source"],
        "declared"
    );
    // cleared: detection again
    let answer = h.declare("proj", "docs/a.md", None).unwrap();
    assert_eq!(
        answer,
        json!({ "langs": ["en"], "source": "detected", "detected": "en" })
    );
    let shared: Value = serde_json::from_slice(&std::fs::read(places.shared()).unwrap()).unwrap();
    assert_eq!(shared["data"]["docs"], json!({}));
}

#[test]
fn a_declared_language_outside_a_project_is_kept_in_the_profile() {
    let mut h = Harness::start();
    let places = h.offer_files();
    h.declare("notes", "n.md", Some("ro")).unwrap();
    assert_eq!(
        h.language("notes", "n.md", ENGLISH, "en")["source"],
        "declared"
    );
    let kept: Value = serde_json::from_slice(&std::fs::read(profile_file(&h)).unwrap()).unwrap();
    let key = places.notes.join("n.md").display().to_string();
    assert_eq!(kept["data"]["docs"][&key], "ro");
    assert!(!places.shared().exists());
    h.restart();
    assert_eq!(
        h.language("notes", "n.md", ENGLISH, "en")["source"],
        "declared"
    );
    h.declare("notes", "n.md", None).unwrap();
    assert_eq!(
        h.language("notes", "n.md", ENGLISH, "en")["source"],
        "detected"
    );
}

#[test]
fn a_declaration_is_refused_outside_the_window_or_for_an_unknown_language() {
    let mut h = Harness::start();
    let places = h.offer_files();
    for (window, path, lang) in [
        ("proj", "docs/missing.md", "ro"),
        ("proj", "../notes/n.md", "ro"),
        ("other", "docs/a.md", "ro"),
        ("proj", "docs/a.md", "pt-BR"),
    ] {
        assert!(
            h.declare(window, path, Some(lang)).is_err(),
            "{window} {path} {lang}"
        );
    }
    assert!(!places.shared().exists());
    assert!(!profile_file(&h).exists());
    // a declared language whose dictionary is gone counts as none
    std::fs::write(
        places.shared(),
        r#"{"version":1,"data":{"docs":{"docs/a.md":"xx"}}}"#,
    )
    .unwrap();
    assert_eq!(
        h.language("proj", "docs/a.md", ENGLISH, "en")["source"],
        "detected"
    );
}

#[test]
fn a_shared_file_this_version_cannot_read_is_never_written_over() {
    let mut h = Harness::start();
    let places = h.offer_files();
    for bytes in [
        &br#"{"version":2,"data":{"docs":{"docs/a.md":"ro"}},"more":1}"#[..],
        b"{ not json",
    ] {
        std::fs::write(places.shared(), bytes).unwrap();
        let refused = h.declare("proj", "docs/b.md", Some("ro")).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::InvalidData);
        assert_eq!(std::fs::read(places.shared()).unwrap(), bytes);
        // reading it finds no declared language, and no crash
        assert_eq!(
            h.language("proj", "docs/a.md", ENGLISH, "en")["source"],
            "detected"
        );
    }
}

#[test]
fn without_the_window_files_service_nothing_is_declared() {
    let h = Harness::start();
    assert_eq!(
        h.language("main", "a.md", ENGLISH, "en")["source"],
        "detected"
    );
    let refused = h.declare("main", "a.md", Some("ro")).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Unavailable);
}
