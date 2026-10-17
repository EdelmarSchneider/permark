//! The look of tags (a color and an icon) kept in `meta.toml` next to the tags themselves.

use super::*;

/// `(tag, color, icon)` of each styled tag.
fn looks(svc: &MetaService) -> Vec<(String, Option<&'static str>, Option<&'static str>)> {
    svc.tag_styles()
        .into_iter()
        .map(|(tag, v)| (tag, v.color, v.icon))
        .collect()
}

fn look(
    tag: &str,
    color: Option<&'static str>,
    icon: Option<&'static str>,
) -> (String, Option<&'static str>, Option<&'static str>) {
    (tag.to_owned(), color, icon)
}

#[test]
fn a_tags_look_is_written_to_the_file_and_read_back_in_both_places() {
    for in_project in [false, true] {
        let fx = Fx::new();
        fx.write("a.md", "# A\n");
        let path = if in_project {
            fx.project_meta()
        } else {
            fx.app_meta()
        };
        let svc = fx.service_at(path.clone(), in_project, QUICK);
        assert_eq!(looks(&svc).len(), 0);
        let before = fx.notified.load(Ordering::SeqCst);
        let styles = svc
            .set_tag_style("Needs Review", Some("orange"), Some("flag"))
            .expect("style");
        assert_eq!(styles["needs-review"].color, Some("orange"));
        assert!(
            fx.notified.load(Ordering::SeqCst) > before,
            "the UI hears of it"
        );
        svc.set_tag_style("spec", Some("blue"), None)
            .expect("style");
        wait_until("the write", || path.exists());
        svc.stop();

        let text = fs::read_to_string(&path).expect("read");
        assert!(
            text.contains("[tag_styles.needs-review]\ncolor = \"orange\"\nicon = \"flag\"\n"),
            "{text}"
        );
        let again = fx.service_at(path, in_project, QUICK);
        assert_eq!(
            looks(&again),
            [
                look("needs-review", Some("orange"), Some("flag")),
                look("spec", Some("blue"), None)
            ]
        );
        again.stop();
    }
}

#[test]
fn clearing_a_tags_look_removes_it_from_the_file() {
    let fx = Fx::new();
    let svc = fx.project_service();
    svc.set_tag_style("spec", Some("red"), Some("star"))
        .expect("style");
    let cleared = svc.set_tag_style("spec", None, None).expect("clear");
    assert!(cleared.is_empty());
    svc.stop();
    let text = fs::read_to_string(fx.project_meta()).expect("read");
    assert!(!text.contains("tag_styles"), "{text}");
}

#[test]
fn a_look_outside_the_palette_is_ignored_and_a_blank_tag_has_none() {
    let fx = Fx::new();
    let svc = fx.project_service();
    let styles = svc
        .set_tag_style("spec", Some("chartreuse"), Some("rocket"))
        .expect("style");
    assert!(styles.is_empty());
    assert!(svc
        .set_tag_style("  ", Some("red"), None)
        .expect("blank")
        .is_empty());
    svc.stop();
    assert!(
        !fx.project_meta().exists(),
        "nothing to keep, nothing written"
    );
}

#[test]
fn looks_that_nothing_uses_any_more_do_no_harm() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write(
        ".permark/meta.toml",
        "[files.\"a.md\"]\ntags = [\"x\"]\n\n[tag_styles.gone]\ncolor = \"green\"\n\n[tag_styles.x]\nicon = \"star\"\n",
    );
    let svc = fx.project_service();
    // the overview lists the tags the files have; a style for another tag is simply not asked about
    let overview = svc.overview();
    let listed: Vec<&str> = overview.tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(listed, ["x"]);
    assert_eq!(looks(&svc).len(), 2);
    // renaming and removing tags elsewhere leaves the styles alone, and nothing crashes
    svc.set_tags("a.md", &s(&["renamed"])).expect("tags");
    assert_eq!(looks(&svc).len(), 2);
    assert!(svc.clean_up().is_ok());
    svc.stop();
}

#[test]
fn a_file_with_unknown_looks_is_read_and_keeps_them_when_rewritten() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write(
        ".permark/meta.toml",
        "[tag_styles.spec]\ncolor = \"chartreuse\"\nicon = \"star\"\n",
    );
    let svc = fx.project_service();
    assert_eq!(
        looks(&svc),
        [look("spec", None, Some("star"))],
        "only what this version knows is shown"
    );
    svc.set_tags("a.md", &s(&["x"])).expect("tags");
    svc.stop();
    let text = fs::read_to_string(fx.project_meta()).expect("read");
    assert!(text.contains("chartreuse"), "{text}");
}

#[test]
fn an_unreadable_file_makes_looks_read_only_and_changes_wait_for_the_fix() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write(".permark/meta.toml", "[tag_styles.old]\ncolor = \"red\"\n");
    let svc = fx.lazy_service(fx.project_meta(), true);
    svc.set_tag_style("mine", Some("teal"), None)
        .expect("style");

    // a git operation leaves conflict markers before our delayed write
    fx.write(".permark/meta.toml", "<<<<<<< HEAD\n[tag_styles.old]\n");
    assert_eq!(svc.on_disk_event(), DiskEvent::Reloaded);
    let status = svc.status();
    assert!(status.problem.is_some());
    assert_eq!(status.pending_changes, 1, "the person is told it waits");
    let err = svc
        .set_tag_style("other", Some("blue"), None)
        .expect_err("read-only");
    assert_eq!(err.kind(), "metaUnreadable");

    fx.write(".permark/meta.toml", "[tag_styles.old]\ncolor = \"red\"\n");
    assert_eq!(svc.on_disk_event(), DiskEvent::Reloaded);
    assert_eq!(svc.status().pending_changes, 0);
    assert_eq!(
        looks(&svc),
        [
            look("mine", Some("teal"), None),
            look("old", Some("red"), None)
        ]
    );
    svc.stop();
    let text = fs::read_to_string(fx.project_meta()).expect("read");
    assert!(
        text.contains("[tag_styles.mine]") && text.contains("[tag_styles.old]"),
        "{text}"
    );
}

#[test]
fn another_edit_of_the_file_changes_the_looks_shown() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    let svc = fx.project_service();
    svc.set_tag_style("spec", Some("blue"), None)
        .expect("style");
    wait_until("the write", || fx.project_meta().exists());
    assert_eq!(svc.on_disk_event(), DiskEvent::Echo);

    fx.write(
        ".permark/meta.toml",
        "[tag_styles.spec]\ncolor = \"purple\"\nicon = \"code\"\n",
    );
    assert_eq!(svc.on_disk_event(), DiskEvent::Reloaded);
    assert_eq!(looks(&svc), [look("spec", Some("purple"), Some("code"))]);
    svc.stop();
}

#[test]
fn initializing_the_project_moves_the_looks_along_and_keeps_the_teams() {
    let fx = Fx::new();
    fx.write("a.md", "# A\n");
    fx.write(
        ".permark/meta.toml",
        "[tag_styles.shared]\ncolor = \"green\"\n[tag_styles.spec]\ncolor = \"blue\"\n",
    );
    let svc = fx.service();
    svc.set_tag_style("spec", Some("red"), Some("flag"))
        .expect("style");
    svc.set_tag_style("mine", None, Some("star"))
        .expect("style");

    svc.migrate_to(fx.project_meta(), true).expect("migrate");
    assert!(!fx.app_meta().exists(), "moved, not copied");
    assert_eq!(
        looks(&svc),
        [
            look("mine", None, Some("star")),
            look("shared", Some("green"), None),
            look("spec", Some("blue"), None)
        ],
        "the project's own look wins, the others are added"
    );
    let text = fs::read_to_string(fx.project_meta()).expect("read");
    assert!(text.contains("[tag_styles.mine]"), "{text}");
    svc.stop();
}
