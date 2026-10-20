use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use super::linux::{install, mimeapps_path, Install, MimeApps, DESKTOP_ID};
use super::*;
use crate::{ErrorKind, IpcError};

#[test]
fn groups_have_distinct_lowercase_extensions_and_only_markdown_is_checked() {
    let mut seen = HashSet::new();
    for group in FileGroup::ALL {
        assert_ne!(group.extensions(), &[] as &[&str]);
        for extension in group.extensions() {
            assert!(extension.starts_with('.'), "{extension}");
            assert_eq!(*extension, extension.to_ascii_lowercase());
            assert!(seen.insert(*extension), "{extension} twice");
        }
        assert!(group.prog_id().starts_with("PerMark."));
        assert_ne!(group.mime_types(), &[] as &[&str]);
        assert_eq!(group.checked_by_default(), group == FileGroup::Markdown);
    }
    let ids: HashSet<_> = FileGroup::ALL.iter().map(|g| g.prog_id()).collect();
    assert_eq!(ids.len(), FileGroup::ALL.len());
    assert_eq!(FileGroup::Markdown.prog_id(), "PerMark.Markdown");
}

// ---- mimeapps.list ------------------------------------------------------------------------------------------------

const LIST: &str = "# mine\r\n[Added Associations]\r\ntext/markdown=other.desktop;\r\n\r\n[Default Applications]\r\ntext/plain=gedit.desktop;\r\n# text/markdown=commented.desktop;\r\ntext/markdown=org.gnome.TextEditor.desktop;\r\n\r\n[Removed Associations]\r\n";

#[test]
fn reads_the_default_of_the_right_section_only() {
    assert_eq!(
        default_of(LIST, "text/markdown").as_deref(),
        Some("org.gnome.TextEditor.desktop;")
    );
    assert_eq!(
        default_of(LIST, "text/plain").as_deref(),
        Some("gedit.desktop;")
    );
    assert_eq!(default_of(LIST, "application/json"), None);
    assert_eq!(default_of("", "text/plain"), None);
    assert_eq!(
        default_of("[Added Associations]\ntext/plain=x;\n", "text/plain"),
        None
    );
    assert_eq!(
        default_of(
            "[Default Applications]\n  text/plain = spaced.desktop ; \n",
            "text/plain"
        )
        .as_deref(),
        Some("spaced.desktop ;")
    );
}

#[test]
fn changing_a_key_changes_only_its_value_and_keeps_crlf() {
    let out = with_default(LIST, "text/markdown", Some("PerMark.desktop;"));
    assert_eq!(
        out,
        LIST.replace(
            "text/markdown=org.gnome.TextEditor.desktop;",
            "text/markdown=PerMark.desktop;"
        )
    );
    // the same key in another section and the commented line stay
    assert!(out.contains("[Added Associations]\r\ntext/markdown=other.desktop;"));
    assert!(out.contains("# text/markdown=commented.desktop;"));
}

#[test]
fn removing_a_key_removes_its_line_only() {
    let out = with_default(LIST, "text/markdown", None);
    assert_eq!(
        out,
        LIST.replace("text/markdown=org.gnome.TextEditor.desktop;\r\n", "")
    );
    // nothing to remove: unchanged
    assert_eq!(with_default(LIST, "application/json", None), LIST);
    assert_eq!(with_default("", "application/json", None), "");
}

#[test]
fn a_new_key_goes_after_the_last_line_of_the_section() {
    let out = with_default(LIST, "application/json", Some("PerMark.desktop;"));
    assert!(out.contains(
        "text/markdown=org.gnome.TextEditor.desktop;\r\napplication/json=PerMark.desktop;\r\n\r\n[Removed Associations]"
    ));
    assert_eq!(
        out.len(),
        LIST.len() + "application/json=PerMark.desktop;\r\n".len()
    );
}

#[test]
fn the_section_is_made_when_missing() {
    assert_eq!(
        with_default("", "text/plain", Some("PerMark.desktop;")),
        "[Default Applications]\ntext/plain=PerMark.desktop;\n"
    );
    assert_eq!(
        with_default("[Added Associations]\r\na=b;", "text/plain", Some("P;")),
        "[Added Associations]\r\na=b;\r\n[Default Applications]\r\ntext/plain=P;\r\n"
    );
}

#[test]
fn a_section_at_the_end_without_a_final_break_gets_the_key() {
    let text = "[Default Applications]\ntext/plain=a;";
    assert_eq!(
        with_default(text, "text/markdown", Some("P;")),
        "[Default Applications]\ntext/plain=a;\ntext/markdown=P;\n"
    );
    assert_eq!(
        with_default(text, "text/plain", Some("P;")),
        "[Default Applications]\ntext/plain=P;"
    );
    assert_eq!(
        with_default("[Default Applications]\n", "text/plain", Some("P;")),
        "[Default Applications]\ntext/plain=P;\n"
    );
}

#[test]
fn every_copy_of_a_repeated_key_changes() {
    let text = "[Default Applications]\nx/y=a;\nx/y=b;\n";
    assert_eq!(
        with_default(text, "x/y", Some("P;")),
        "[Default Applications]\nx/y=P;\nx/y=P;\n"
    );
    assert_eq!(with_default(text, "x/y", None), "[Default Applications]\n");
}

#[test]
fn round_trip_gives_the_file_back_byte_for_byte() {
    for text in [
        LIST,
        "",
        "[Default Applications]\n",
        "[Default Applications]\ntext/plain=a;",
    ] {
        for mime in ["text/markdown", "text/plain", "application/json"] {
            let before = default_of(text, mime);
            let set = with_default(text, mime, Some("PerMark.desktop;"));
            let back = with_default(&set, mime, before.as_deref());
            if before.is_some() {
                assert_eq!(back, text, "{mime} in {text:?}");
            } else {
                assert_eq!(default_of(&back, mime), None);
            }
        }
    }
}

// ---- choosing, over a fake system --------------------------------------------------------------------------------

struct Fake {
    table: RefCell<BTreeMap<String, String>>,
    refuse: Option<&'static str>,
}

impl Fake {
    fn new(entries: &[(&str, &str)]) -> Self {
        Self {
            table: RefCell::new(
                entries
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            ),
            refuse: None,
        }
    }
}

impl Handlers for Fake {
    fn keys(&self, group: FileGroup) -> Vec<String> {
        group.mime_types().iter().map(|m| (*m).to_owned()).collect()
    }
    fn get(&self, key: &str) -> Option<String> {
        self.table.borrow().get(key).cloned()
    }
    fn set(&self, key: &str, handler: Option<&str>) -> Result<(), IpcError> {
        if self.refuse == Some(key) {
            return Err(IpcError::new(ErrorKind::Denied));
        }
        match handler {
            Some(h) => self.table.borrow_mut().insert(key.to_owned(), h.to_owned()),
            None => self.table.borrow_mut().remove(key),
        };
        Ok(())
    }
    fn me(&self) -> &'static str {
        "PerMark.desktop"
    }
}

#[test]
fn choosing_records_the_previous_handler_and_unchoosing_gives_it_back() {
    let fake = Fake::new(&[("text/markdown", "editor.desktop;")]);
    let mut state = DefaultAppState::default();
    assert!(!is_ours(&fake, FileGroup::Markdown));
    choose(&fake, &mut state, FileGroup::Markdown, true).unwrap();
    assert!(is_ours(&fake, FileGroup::Markdown));
    assert_eq!(
        state.previous["text/markdown"].as_deref(),
        Some("editor.desktop;")
    );
    assert_eq!(state.previous["text/x-markdown"], None);
    // again: the first record stays
    choose(&fake, &mut state, FileGroup::Markdown, true).unwrap();
    assert_eq!(
        state.previous["text/markdown"].as_deref(),
        Some("editor.desktop;")
    );
    choose(&fake, &mut state, FileGroup::Markdown, false).unwrap();
    assert_eq!(
        fake.get("text/markdown").as_deref(),
        Some("editor.desktop;")
    );
    assert_eq!(fake.get("text/x-markdown"), None);
    assert!(state.previous.is_empty());
}

#[test]
fn unchoosing_keeps_a_choice_made_elsewhere_since() {
    let fake = Fake::new(&[]);
    let mut state = DefaultAppState::default();
    choose(&fake, &mut state, FileGroup::Json, true).unwrap();
    fake.set("application/json", Some("code.desktop;")).unwrap();
    choose(&fake, &mut state, FileGroup::Json, false).unwrap();
    assert_eq!(
        fake.get("application/json").as_deref(),
        Some("code.desktop;")
    );
    assert!(state.previous.is_empty());
}

#[test]
fn a_refused_key_stops_with_the_keys_before_it_recorded() {
    let mut fake = Fake::new(&[("application/xml", "a;"), ("text/xml", "b;")]);
    fake.refuse = Some("text/xml");
    let mut state = DefaultAppState::default();
    let error = choose(&fake, &mut state, FileGroup::Xml, true).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Denied);
    assert_eq!(
        fake.get("application/xml").as_deref(),
        Some("PerMark.desktop")
    );
    assert_eq!(state.previous["application/xml"].as_deref(), Some("a;"));
    // the previous of the refused key is recorded too, so a later "off" can still give it back
    assert_eq!(state.previous["text/xml"].as_deref(), Some("b;"));
    fake.refuse = None;
    choose(&fake, &mut state, FileGroup::Xml, false).unwrap();
    assert_eq!(fake.get("application/xml").as_deref(), Some("a;"));
    assert_eq!(fake.get("text/xml").as_deref(), Some("b;"));
}

#[test]
fn the_state_round_trips_through_json() {
    let mut state = DefaultAppState {
        asked: true,
        previous: BTreeMap::new(),
    };
    state.previous.insert("text/markdown".into(), None);
    state
        .previous
        .insert("text/plain".into(), Some("gedit.desktop;".into()));
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(
        serde_json::from_str::<DefaultAppState>(&json).unwrap(),
        state
    );
    // older files without `previous`
    let bare: DefaultAppState = serde_json::from_str(r#"{"asked":false}"#).unwrap();
    assert!(bare.previous.is_empty());
}

// ---- Linux files --------------------------------------------------------------------------------------------------

#[test]
fn mimeapps_on_disk_round_trip_in_a_folder_with_spaces_and_accents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cönfig dir").join("mimeapps.list");
    let apps = MimeApps::new(path.clone(), DESKTOP_ID);
    let mut state = DefaultAppState::default();
    choose(&apps, &mut state, FileGroup::Markdown, true).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        "[Default Applications]\ntext/markdown=PerMark.desktop;\ntext/x-markdown=PerMark.desktop;\n"
    );
    assert!(is_ours(&apps, FileGroup::Markdown));
    choose(&apps, &mut state, FileGroup::Markdown, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[Default Applications]\n"
    );
}

#[test]
fn mimeapps_keeps_the_rest_of_the_persons_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mimeapps.list");
    std::fs::write(&path, LIST).unwrap();
    let apps = MimeApps::new(path.clone(), DESKTOP_ID);
    let mut state = DefaultAppState::default();
    choose(&apps, &mut state, FileGroup::Markdown, true).unwrap();
    choose(&apps, &mut state, FileGroup::Markdown, false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LIST);
}

#[test]
fn mimeapps_that_is_not_utf8_is_refused_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mimeapps.list");
    std::fs::write(&path, b"[Default Applications]\n\xff\xfe=x\n").unwrap();
    let apps = MimeApps::new(path.clone(), DESKTOP_ID);
    let error = apps.set("text/plain", Some(DESKTOP_ID)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Encoding);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"[Default Applications]\n\xff\xfe=x\n"
    );
}

fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| (*v).to_owned())
    }
}

#[test]
fn linux_install_kinds() {
    let nothing = |_: &Path| false;
    let system = |p: &Path| p == Path::new("/usr/share/applications/PerMark.desktop");
    assert_eq!(
        install(env(&[("SNAP", "/snap/permark/3")]), system),
        Install::Snap
    );
    assert_eq!(
        install(env(&[("APPIMAGE", "/home/a/PerMark.AppImage")]), system),
        Install::Loose
    );
    assert_eq!(
        install(env(&[("HOME", "/home/a")]), system),
        Install::Desktop
    );
    assert_eq!(
        install(env(&[("HOME", "/home/a")]), nothing),
        Install::Loose
    );
    // a desktop file of the person's own
    let local = |p: &Path| p == Path::new("/home/a/.local/share/applications/PerMark.desktop");
    assert_eq!(
        install(env(&[("HOME", "/home/a")]), local),
        Install::Desktop
    );
    // relative entries in XDG variables are ignored
    let odd = |p: &Path| p.is_relative();
    assert_eq!(
        install(
            env(&[("XDG_DATA_DIRS", "share:rel"), ("XDG_DATA_HOME", "x")]),
            odd
        ),
        Install::Loose
    );
    // empty variables are not set
    assert_eq!(
        install(env(&[("SNAP", ""), ("HOME", "/home/a")]), system),
        Install::Desktop
    );
}

#[test]
fn mimeapps_path_follows_xdg() {
    assert_eq!(
        mimeapps_path(env(&[("XDG_CONFIG_HOME", "/c"), ("HOME", "/h")])),
        Some(Path::new("/c").join("mimeapps.list"))
    );
    assert_eq!(
        mimeapps_path(env(&[("XDG_CONFIG_HOME", "rel"), ("HOME", "/h")])),
        Some(Path::new("/h").join(".config").join("mimeapps.list"))
    );
    assert_eq!(mimeapps_path(env(&[])), None);
}

#[test]
fn asking_once_and_only_when_it_can_act() {
    let fresh = DefaultAppState::default();
    let asked = DefaultAppState {
        asked: true,
        ..DefaultAppState::default()
    };
    let unsupported = DefaultAppMode::Unsupported {
        reason: Unsupported::Confined,
    };
    assert!(should_ask(&fresh, false, &DefaultAppMode::Direct));
    assert!(should_ask(&fresh, false, &DefaultAppMode::Handoff));
    assert!(!should_ask(&asked, false, &DefaultAppMode::Direct));
    assert!(
        !should_ask(&fresh, true, &DefaultAppMode::Direct),
        "an older PerMark asked before"
    );
    assert!(!should_ask(&fresh, false, &unsupported));
}

#[test]
fn the_mode_is_told_by_name() {
    assert_eq!(
        serde_json::to_value(DefaultAppMode::Direct).unwrap(),
        serde_json::json!({ "mode": "direct" })
    );
    assert_eq!(
        serde_json::to_value(DefaultAppMode::Unsupported {
            reason: Unsupported::NotInstalled
        })
        .unwrap(),
        serde_json::json!({ "mode": "unsupported", "reason": "notInstalled" })
    );
}

#[cfg(windows)]
#[test]
fn windows_prog_ids_of_permark() {
    use super::windows::is_ours;
    let none = |_: &str| None;
    assert!(is_ours("PerMark.Markdown", none));
    assert!(is_ours("permark.json", none));
    assert!(!is_ours("VSCode.md", none));
    assert!(!is_ours("PerMarkX", none));
    let store = |_: &str| Some("EdelmarSchneider.TrailMark_abc123!PerMark".to_owned());
    assert!(is_ours("AppXk3j2", store));
    let other = |_: &str| Some("Microsoft.WindowsNotepad_8wekyb!App".to_owned());
    assert!(!is_ours("AppXk3j2", other));
    assert!(!is_ours("NotAppX", store));
}
