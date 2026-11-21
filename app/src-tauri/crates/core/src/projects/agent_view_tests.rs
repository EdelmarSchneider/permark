//! Rule files read checked: any Unicode form a Windows editor | shell writes, an unreadable file standing on its last
//! good rules (kept in the profile only) or closing what it governs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::edit::lines::encode;
use crate::fsx::Encoding;
use crate::projects::{AncestorRules, HiddenRule, MemberInfo, ProjectMarker, ProjectUid};
use crate::state::{AppDirs, PROFILE_ONLY};

const RULES: &str = "private/\n";

/// `private/` as UTF-8 with a BOM (Notepad), UTF-16 LE (PowerShell 5.1 `>`) and UTF-16 BE, each with its BOM.
fn forms() -> [(Encoding, Vec<u8>); 3] {
    [Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be]
        .map(|form| (form, encode(RULES, form)))
}

fn put(base: &Path, rel: &str, bytes: &[u8]) {
    let file = base.join(rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}

/// A project folder with a shared and a private document.
fn project() -> (tempfile::TempDir, PathBuf, Root) {
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    put(&base, "docs/a.md", b"# A\n");
    put(&base, "private/s.md", b"# S\n");
    let root = Root::open(&base).unwrap();
    (dir, base, root)
}

fn store_for(profile: &Path, root: &Root) -> StateStore {
    StateStore::for_project(&AppDirs::under(profile), root)
}

fn problem(file: &str, why: RulesWhy, fallback: RulesFallback) -> RulesProblem {
    RulesProblem {
        file: file.to_owned(),
        why,
        fallback,
    }
}

fn member(path: &str, agent: AgentRules) -> MemberInfo {
    let path = VaultPath::parse(path).unwrap();
    MemberInfo {
        uid: ProjectUid::parse("aaaaaaaaaa").unwrap(),
        key: path.key(),
        path,
        marker: ProjectMarker::Git,
        hidden: HiddenRule::Own,
        name: String::new(),
        short: String::new(),
        agent_off: false,
        agent,
        first_commit: None,
    }
}

fn map(members: Vec<MemberInfo>, ancestors: Vec<AncestorRules>) -> Arc<ProjectMap> {
    Arc::new(ProjectMap::new(1, true, members, Vec::new(), ancestors))
}

#[test]
fn a_rule_file_in_any_unicode_form_hides_for_the_root() {
    for (form, bytes) in forms() {
        let (_dir, base, root) = project();
        put(&base, AI_IGNORE, &bytes);
        assert_eq!(
            read_rules(&base.join(AI_IGNORE)),
            RuleRead::Read(RULES.to_owned()),
            "{form:?}"
        );
        let guard = Guard::new(&root, &[]);
        assert!(guard.hidden("private/s.md", false), "{form:?}");
        assert!(!guard.hidden("docs/a.md", false), "{form:?}");
        assert!(
            !guard.closed() && guard.rules_problems().is_empty(),
            "{form:?}"
        );
    }
}

#[test]
fn a_member_and_an_ancestor_rule_file_in_any_unicode_form_hide() {
    for (form, bytes) in forms() {
        // a member's own `.aiignore`, matched member-relative
        let (_dir, base, root) = project();
        put(&base, "sub/.aiignore", &bytes);
        put(&base, "sub/private/x.md", b"# X\n");
        let rules = folder_rules(&base.join("sub"), "sub", ConfigRead::Absent, None);
        assert_eq!(rules.aiignore, ["private/"], "{form:?}");
        let guard =
            Guard::new(&root, &[]).with_projects(map(vec![member("sub", rules)], Vec::new()), None);
        assert!(guard.hidden("sub/private/x.md", false), "{form:?}");
        assert!(
            !guard.hidden("private/s.md", false),
            "{form:?}: the member's rule stays inside it"
        );
        // an ancestor's `.aiignore` (opened alone below it)
        let (_up_dir, up, _) = project();
        put(&up, AI_IGNORE, &bytes);
        let opened = up.join("proj");
        put(&opened, "private/x.md", b"# X\n");
        put(&opened, "docs/a.md", b"# A\n");
        let ancestor = AncestorRules {
            rules: folder_rules(&up, "..", ConfigRead::Absent, None),
            base: up.clone(),
            rel: "proj".to_owned(),
        };
        let guard = Guard::new(&Root::open(&opened).unwrap(), &[])
            .with_projects(map(Vec::new(), vec![ancestor]), None);
        assert!(guard.hidden("private/x.md", false), "{form:?}");
        assert!(!guard.hidden("docs/a.md", false), "{form:?}");
    }
}

#[test]
fn a_too_large_rule_file_closes_the_guard_until_it_has_last_good_rules() {
    let (_dir, base, root) = project();
    let profile = tempfile::tempdir().unwrap();
    let store = store_for(profile.path(), &root);
    let big = format!("{RULES}{}", "#".repeat(1_153_434));
    put(&base, AI_IGNORE, big.as_bytes());
    let guard = Guard::checked(&root, &store);
    assert!(guard.closed());
    assert!(guard.hidden("docs/a.md", false), "closed: nothing shared");
    assert_eq!(guard.file("docs/a.md"), Err(Refusal::Missing));
    assert_eq!(
        guard.rules_problems(),
        [problem(
            AI_IGNORE,
            RulesWhy::TooLarge,
            RulesFallback::Closed
        )]
    );
    // a good read keeps its rules; too large again, the file stands on them
    put(&base, AI_IGNORE, RULES.as_bytes());
    let guard = Guard::checked(&root, &store);
    assert!(!guard.closed() && guard.rules_problems().is_empty());
    assert!(guard.hidden("private/s.md", false));
    put(&base, AI_IGNORE, big.as_bytes());
    let guard = Guard::checked(&root, &store);
    assert!(!guard.closed());
    assert!(guard.hidden("private/s.md", false), "last good rules");
    assert!(!guard.hidden("docs/a.md", false));
    assert!(guard.file("docs/a.md").is_ok());
    assert_eq!(
        guard.rules_problems(),
        [problem(
            AI_IGNORE,
            RulesWhy::TooLarge,
            RulesFallback::LastGood
        )]
    );
}

#[test]
fn a_rule_file_that_is_a_folder_or_undecodable_or_unbuildable_is_unreadable() {
    let (_dir, base, root) = project();
    let file = base.join(AI_IGNORE);
    assert_eq!(read_rules(&file), RuleRead::Absent);
    fs::create_dir(&file).unwrap();
    assert_eq!(read_rules(&file), RuleRead::Unreadable(RulesWhy::NotAFile));
    let guard = Guard::new(&root, &[]);
    assert!(guard.closed() && guard.hidden("docs/a.md", false));
    assert_eq!(
        guard.rules_problems(),
        [problem(
            AI_IGNORE,
            RulesWhy::NotAFile,
            RulesFallback::Closed
        )]
    );
    fs::remove_dir(&file).unwrap();
    // invalid UTF-8 would turn a pattern into one that matches nothing; NULs without a UTF-16 BOM are no text
    for bytes in [&b"priv\xFFate/\n"[..], b"private/\0\n"] {
        fs::write(&file, bytes).unwrap();
        assert_eq!(read_rules(&file), RuleRead::Unreadable(RulesWhy::NotText));
    }
    // a pattern that does not build would leave what it names shared
    fs::write(&file, "private/\ndocs/[z-a]\n").unwrap();
    let guard = Guard::new(&root, &[]);
    assert!(guard.closed());
    assert_eq!(
        guard.rules_problems(),
        [problem(
            AI_IGNORE,
            RulesWhy::BadPattern,
            RulesFallback::Closed
        )]
    );
    fs::write(&file, RULES).unwrap();
    let guard = Guard::new(&root, &["docs/[z-a]".to_owned()]);
    assert!(guard.closed());
    assert_eq!(
        guard.rules_problems(),
        [problem(
            CONFIG_FILE,
            RulesWhy::BadPattern,
            RulesFallback::Closed
        )]
    );
}

#[test]
fn a_team_file_left_in_a_merge_conflict_stands_on_its_last_good_rules() {
    let (_dir, base, root) = project();
    let good = "[agent]\ndeny_paths = [\"private/\"]\ndeny = [\"find\"]\n";
    let conflict = "<<<<<<< HEAD\n[agent]\ndeny_paths = [\"private/\"]\n=======\n>>>>>>> theirs\n";
    // never read well: closed
    put(&base, CONFIG_FILE, conflict.as_bytes());
    let fresh = tempfile::tempdir().unwrap();
    let guard = Guard::checked(&root, &store_for(fresh.path(), &root));
    assert!(guard.closed() && guard.hidden("docs/a.md", false));
    assert_eq!(
        guard.rules_problems(),
        [problem(
            CONFIG_FILE,
            RulesWhy::NotToml,
            RulesFallback::Closed
        )]
    );
    // read well once: its rules and denied groups stand in for the broken file
    let profile = tempfile::tempdir().unwrap();
    let store = store_for(profile.path(), &root);
    put(&base, CONFIG_FILE, good.as_bytes());
    let guard = Guard::checked(&root, &store);
    assert!(guard.hidden("private/s.md", false));
    assert_eq!(guard.denied_groups().iter().collect::<Vec<_>>(), ["find"]);
    put(&base, CONFIG_FILE, conflict.as_bytes());
    let guard = Guard::checked(&root, &store);
    assert!(!guard.closed());
    assert!(guard.hidden("private/s.md", false));
    assert!(!guard.hidden("docs/a.md", false));
    assert_eq!(guard.denied_groups().iter().collect::<Vec<_>>(), ["find"]);
    assert_eq!(
        guard.rules_problems(),
        [problem(
            CONFIG_FILE,
            RulesWhy::NotToml,
            RulesFallback::LastGood
        )]
    );
    // too large and not UTF-8 count as unreadable too, never as no rules
    put(
        &base,
        CONFIG_FILE,
        &[b"[agent]\ndeny_paths = [\"private/\"]\n# \xFF\n".as_slice()].concat(),
    );
    assert_eq!(
        Guard::checked(&root, &store).rules_problems(),
        [problem(
            CONFIG_FILE,
            RulesWhy::NotText,
            RulesFallback::LastGood
        )]
    );
    put(
        &base,
        CONFIG_FILE,
        format!("{good}#{}\n", "x".repeat(1_100_000)).as_bytes(),
    );
    assert_eq!(
        Guard::checked(&root, &store).rules_problems(),
        [problem(
            CONFIG_FILE,
            RulesWhy::TooLarge,
            RulesFallback::LastGood
        )]
    );
}

#[test]
fn an_unreadable_member_file_hides_the_member_and_an_unreadable_ancestor_file_closes_the_guard() {
    let (_dir, base, root) = project();
    put(&base, "sub/doc.md", b"# D\n");
    fs::create_dir_all(base.join("sub").join(AI_IGNORE)).unwrap();
    let rules = folder_rules(&base.join("sub"), "sub", ConfigRead::Absent, None);
    assert_eq!(
        rules.problems,
        [problem(
            "sub/.aiignore",
            RulesWhy::NotAFile,
            RulesFallback::Closed
        )]
    );
    let guard =
        Guard::new(&root, &[]).with_projects(map(vec![member("sub", rules)], Vec::new()), None);
    assert!(guard.hidden("sub/doc.md", false) && guard.hidden("sub", true));
    assert!(!guard.hidden("docs/a.md", false));
    assert!(!guard.closed(), "only the member's own part");
    assert_eq!(
        guard.rules_problems(),
        [problem(
            "sub/.aiignore",
            RulesWhy::NotAFile,
            RulesFallback::Closed
        )]
    );
    // with last good rules, the member stands on them
    let profile = tempfile::tempdir().unwrap();
    let kept = LastGood::of(&store_for(profile.path(), &root), root.path());
    kept.keep("sub/.aiignore", Some(RULES));
    let rules = folder_rules(&base.join("sub"), "sub", ConfigRead::Absent, Some(&kept));
    assert_eq!(rules.aiignore, ["private/"]);
    let guard =
        Guard::new(&root, &[]).with_projects(map(vec![member("sub", rules)], Vec::new()), None);
    assert!(!guard.hidden("sub/doc.md", false) && guard.hidden("sub/private/x.md", false));
    // an ancestor governs the whole folder: closed
    let opened = base.join("docs");
    let ancestor = AncestorRules {
        rules: folder_rules(&base.join("sub"), "..", ConfigRead::Absent, None),
        base: base.join("sub"),
        rel: "docs".to_owned(),
    };
    assert_eq!(
        ancestor.rules.problems,
        [problem(
            "../.aiignore",
            RulesWhy::NotAFile,
            RulesFallback::Closed
        )]
    );
    let guard = Guard::new(&Root::open(&opened).unwrap(), &[])
        .with_projects(map(Vec::new(), vec![ancestor]), None);
    assert!(guard.closed() && guard.hidden("a.md", false));
}

/// The one rule file told first (the tools' refusal, the settings' line): one that closes something before one standing
/// on last good rules; with the guard closed, the file that closed it, before a member's.
#[test]
fn the_problem_told_first_is_the_one_keeping_agents_out() {
    let (_dir, base, root) = project();
    let lasting = problem(AI_IGNORE, RulesWhy::TooLarge, RulesFallback::LastGood);
    let member_closed = problem("sub/.aiignore", RulesWhy::NotAFile, RulesFallback::Closed);
    let mut own = AgentRules::default();
    own.problems.push(lasting.clone());
    let mut sub = AgentRules::default();
    sub.problems.push(member_closed.clone());
    let guard = Guard::of_rules(&root, own.clone());
    assert_eq!(guard.rules_problem(), Some(&lasting));
    let guard = Guard::of_rules(&root, own.clone())
        .with_projects(map(vec![member("sub", sub.clone())], Vec::new()), None);
    assert!(!guard.closed());
    assert_eq!(guard.rules_problem(), Some(&member_closed));
    // an ancestor closing the folder comes before the member
    let up_closed = problem("../.aiignore", RulesWhy::Locked, RulesFallback::Closed);
    let mut up = AgentRules::default();
    up.problems.push(up_closed.clone());
    let ancestor = AncestorRules {
        rules: up,
        base: base.clone(),
        rel: String::new(),
    };
    let guard = Guard::of_rules(&root, own)
        .with_projects(map(vec![member("sub", sub)], vec![ancestor]), None);
    assert!(guard.closed());
    assert_eq!(guard.rules_problem(), Some(&up_closed));
    assert_eq!(guard.rules_problems(), [lasting, up_closed, member_closed]);
}

#[test]
fn last_good_rules_live_in_the_profile_never_in_the_folder() {
    assert!(PROFILE_ONLY.contains(&LAST_GOOD_FILE));
    let (_dir, base, root) = project();
    // a team folder holding the private state: the last good rules still go to the profile
    fs::create_dir_all(base.join(".permark/local")).unwrap();
    let profile = tempfile::tempdir().unwrap();
    let dirs = AppDirs::under(profile.path());
    let store = StateStore::for_project(&dirs, &root);
    put(&base, AI_IGNORE, RULES.as_bytes());
    let _ = Guard::checked(&root, &store);
    let kept = StateStore::profile_only(&dirs, &root)
        .dir()
        .join(LAST_GOOD_FILE);
    assert!(kept.starts_with(profile.path()) && kept.is_file());
    for folder in [
        base.clone(),
        base.join(".permark"),
        base.join(".permark/local"),
    ] {
        assert!(
            !folder.join(LAST_GOOD_FILE).exists(),
            "{}",
            folder.display()
        );
    }
    // a file of that name arriving with the folder never stands in for the broken rules
    put(
        &base,
        &format!(".permark/local/{LAST_GOOD_FILE}"),
        br#"{"version":1,"data":{"files":[{"file":".aiignore","text":""}]}}"#,
    );
    fs::remove_file(base.join(AI_IGNORE)).unwrap();
    fs::create_dir(base.join(AI_IGNORE)).unwrap();
    let guard = Guard::checked(&root, &store);
    assert!(guard.hidden("private/s.md", false), "the profile's rules");
    assert!(!guard.hidden("docs/a.md", false));
}

#[test]
fn a_rule_file_that_went_away_leaves_no_last_good_rules() {
    let (_dir, base, root) = project();
    let profile = tempfile::tempdir().unwrap();
    let store = store_for(profile.path(), &root);
    put(&base, AI_IGNORE, RULES.as_bytes());
    let _ = Guard::checked(&root, &store);
    fs::remove_file(base.join(AI_IGNORE)).unwrap();
    assert!(!Guard::checked(&root, &store).hidden("private/s.md", false));
    fs::create_dir(base.join(AI_IGNORE)).unwrap();
    let guard = Guard::checked(&root, &store);
    assert!(guard.closed());
    assert_eq!(
        guard.rules_problems(),
        [problem(
            AI_IGNORE,
            RulesWhy::NotAFile,
            RulesFallback::Closed
        )]
    );
}

#[test]
fn the_last_good_store_writes_only_changes_and_stays_bounded() {
    let (_dir, _base, root) = project();
    let profile = tempfile::tempdir().unwrap();
    let store = store_for(profile.path(), &root);
    let kept = LastGood::of(&store, root.path());
    let path = kept.file.path().to_path_buf();
    kept.keep(AI_IGNORE, Some(RULES));
    // the same text again writes nothing (a compact copy of the same data stays as it is)
    let folder = serde_json::to_string(&FolderIdentity::of(root.path()).unwrap()).unwrap();
    let compact = format!(
        r#"{{"version":1,"data":{{"folder":{folder},"files":[{{"file":".aiignore","text":"private/\n"}}]}}}}"#
    );
    fs::write(&path, &compact).unwrap();
    kept.keep(AI_IGNORE, Some(RULES));
    assert_eq!(fs::read_to_string(&path).unwrap(), compact);
    // at most so many files: the oldest written go first
    let mut many = Kept {
        folder: None,
        files: (0..MAX_KEPT_FILES + 4)
            .map(|i| KeptText {
                file: format!("m{i}/.aiignore"),
                text: RULES.to_owned(),
            })
            .collect(),
    };
    many.fit();
    assert_eq!(many.files.len(), MAX_KEPT_FILES);
    assert_eq!(many.text("m3/.aiignore"), None);
    assert_eq!(many.text("m4/.aiignore"), Some(RULES));
    // never larger than its reader takes: big files push the oldest out, the newest stays
    let big = "x".repeat(1024 * 1024);
    for i in 0..6 {
        kept.keep(&format!("big{i}/.aiignore"), Some(&big));
    }
    assert!(fs::metadata(&path).unwrap().len() <= Kept::MAX_BYTES);
    assert_eq!(
        kept.text_of("big5/.aiignore").as_deref(),
        Some(big.as_str())
    );
    assert_eq!(kept.text_of("big0/.aiignore"), None);
    // a file gone: its entry goes
    kept.keep("big5/.aiignore", None);
    assert_eq!(kept.text_of("big5/.aiignore"), None);
}

/// Makes `link` point at the folder `target` (which may be missing): a symbolic link, a junction on Windows (no
/// privilege needed). `false`: not possible here.
fn link_dir(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

/// The lenient read drops an `[agent]` value of the wrong kind; the rules read never takes that as "no rules".
#[test]
fn a_team_value_of_the_wrong_kind_is_unreadable_never_no_rules() {
    let (_dir, base, root) = project();
    let good = "ignore = [\"drafts/\"]\n[agent]\ndeny_paths = [\"private/\"]\n";
    for bad in [
        "ignore = [\"drafts/\"]\n[agent]\ndeny_paths = \"private/\"\n",
        "ignore = [\"drafts/\"]\n[agent]\ndeny = \"changes\"\n",
        "ignore = [\"drafts/\"]\n[agent]\ndeny_paths = [\"private/\", 1]\n",
        "ignore = [\"drafts/\"]\nagent = \"x\"\n",
        "ignore = [\"drafts/\"]\n[[agent]]\ndeny_paths = [\"private/\"]\n",
    ] {
        put(&base, CONFIG_FILE, bad.as_bytes());
        let fresh = tempfile::tempdir().unwrap();
        let guard = Guard::checked(&root, &store_for(fresh.path(), &root));
        assert!(guard.closed() && guard.hidden("docs/a.md", false), "{bad}");
        assert_eq!(
            guard.rules_problems(),
            [problem(
                CONFIG_FILE,
                RulesWhy::BadValue,
                RulesFallback::Closed
            )],
            "{bad}"
        );
        // the settings page still reads what it can
        assert_eq!(ProjectConfig::load(&root).ignore, ["drafts/"], "{bad}");
        // read well once: its last good rules stand in
        let profile = tempfile::tempdir().unwrap();
        let store = store_for(profile.path(), &root);
        put(&base, CONFIG_FILE, good.as_bytes());
        assert!(Guard::checked(&root, &store).hidden("private/s.md", false));
        put(&base, CONFIG_FILE, bad.as_bytes());
        let guard = Guard::checked(&root, &store);
        assert!(!guard.closed(), "{bad}");
        assert!(guard.hidden("private/s.md", false), "{bad}");
        assert!(!guard.hidden("docs/a.md", false), "{bad}");
        assert_eq!(
            guard.rules_problems(),
            [problem(
                CONFIG_FILE,
                RulesWhy::BadValue,
                RulesFallback::LastGood
            )],
            "{bad}"
        );
    }
    // an inline table is a table
    put(
        &base,
        CONFIG_FILE,
        b"agent = { deny_paths = [\"private/\"] }\n",
    );
    let fresh = tempfile::tempdir().unwrap();
    let guard = Guard::checked(&root, &store_for(fresh.path(), &root));
    assert!(!guard.closed() && guard.hidden("private/s.md", false));
}

/// A `.aiignore` that is a link leading nowhere is there: unreadable, its last good rules kept, never erased.
#[test]
fn a_rule_file_link_leading_nowhere_keeps_its_last_good_rules() {
    let (_dir, base, root) = project();
    let profile = tempfile::tempdir().unwrap();
    let store = store_for(profile.path(), &root);
    put(&base, AI_IGNORE, RULES.as_bytes());
    assert!(Guard::checked(&root, &store).hidden("private/s.md", false));
    fs::remove_file(base.join(AI_IGNORE)).unwrap();
    if !link_dir(&base.join(AI_IGNORE), &base.join("missing")) {
        eprintln!("skipped: links cannot be made here");
        return;
    }
    assert_eq!(
        read_rules(&base.join(AI_IGNORE)),
        RuleRead::Unreadable(RulesWhy::NotAFile)
    );
    for _ in 0..2 {
        let guard = Guard::checked(&root, &store);
        assert!(!guard.closed());
        assert!(guard.hidden("private/s.md", false), "last good rules kept");
        assert_eq!(
            guard.rules_problems(),
            [problem(
                AI_IGNORE,
                RulesWhy::NotAFile,
                RulesFallback::LastGood
            )]
        );
    }
}

/// Last good rules belong to the folder they were read in: another folder made at the same path (deleted and made
/// again, cloned anew) never stands on them.
#[test]
fn last_good_rules_never_stand_in_for_another_folder_at_the_same_path() {
    let (_dir, base, root) = project();
    let profile = tempfile::tempdir().unwrap();
    let store = store_for(profile.path(), &root);
    put(&base, AI_IGNORE, RULES.as_bytes());
    assert!(Guard::checked(&root, &store).hidden("private/s.md", false));
    let before = fsx::FolderIdentity::of(&base);
    fs::remove_dir_all(&base).unwrap();
    fs::create_dir(&base).unwrap();
    let after = fsx::FolderIdentity::of(&base);
    if before.zip(after).is_none_or(|(a, b)| a.same(b)) {
        eprintln!("skipped: the folder made again kept its identity here");
        return;
    }
    put(&base, "docs/a.md", b"# A\n");
    fs::create_dir(base.join(AI_IGNORE)).unwrap();
    let root = Root::open(&base).unwrap();
    let store = store_for(profile.path(), &root);
    let guard = Guard::checked(&root, &store);
    assert!(guard.closed() && guard.hidden("docs/a.md", false));
    assert_eq!(
        guard.rules_problems(),
        [problem(
            AI_IGNORE,
            RulesWhy::NotAFile,
            RulesFallback::Closed
        )]
    );
    // a good read of the new folder is kept for it
    fs::remove_dir(base.join(AI_IGNORE)).unwrap();
    put(&base, AI_IGNORE, b"notes/\n");
    assert!(!Guard::checked(&root, &store).closed());
    fs::remove_file(base.join(AI_IGNORE)).unwrap();
    fs::create_dir(base.join(AI_IGNORE)).unwrap();
    let guard = Guard::checked(&root, &store);
    assert!(!guard.closed() && guard.hidden("notes/n.md", false));
}

/// A `.permark` that is a link may hold a team file: its rules are unknown, never "none".
#[test]
fn a_team_folder_that_is_a_link_is_unreadable_for_the_rules() {
    let (_dir, base, root) = project();
    let elsewhere = tempfile::tempdir().unwrap();
    put(
        elsewhere.path(),
        "config.toml",
        b"[agent]\ndeny_paths = [\"private/\"]\n",
    );
    if !link_dir(&base.join(".permark"), elsewhere.path()) {
        eprintln!("skipped: links cannot be made here");
        return;
    }
    let fresh = tempfile::tempdir().unwrap();
    let guard = Guard::checked(&root, &store_for(fresh.path(), &root));
    assert!(guard.closed() && guard.hidden("docs/a.md", false));
    assert_eq!(
        guard.rules_problems(),
        [problem(
            CONFIG_FILE,
            RulesWhy::NotAFile,
            RulesFallback::Closed
        )]
    );
    // the settings page reads nothing from it, as before
    assert_eq!(ProjectConfig::load(&root), ProjectConfig::default());
}

/// Rule files and machinery folders are kept from agents in any case, on every system (only ever narrower).
#[test]
fn private_names_are_hidden_in_any_case_on_every_system() {
    let (_dir, _base, root) = project();
    let guard = Guard::new(&root, &[]);
    for path in [
        ".AIIGNORE",
        "sub/.AiIgnore",
        ".GIT/config",
        "sub/.Git/HEAD",
        ".PERMARK/config.toml",
    ] {
        assert!(guard.hidden(path, false), "{path}");
    }
}

/// An edit that adds a line and breaks another: what builds of it hides beside the last good rules, so the added
/// line is never lost to a typo elsewhere; its `!` lines never un-hide anything.
#[test]
fn an_edit_with_a_bad_pattern_adds_its_good_lines_to_the_last_good_rules() {
    let (_dir, base, root) = project();
    let profile = tempfile::tempdir().unwrap();
    let store = store_for(profile.path(), &root);
    put(&base, AI_IGNORE, RULES.as_bytes());
    assert!(!Guard::checked(&root, &store).hidden("secret/x.md", false));
    put(&base, AI_IGNORE, b"secret/\ndocs/[z-a]\n!private/keep.md\n");
    let guard = Guard::checked(&root, &store);
    assert!(!guard.closed());
    assert!(guard.hidden("secret/x.md", false), "the added line");
    assert!(guard.hidden("private/s.md", false), "the last good rules");
    assert!(guard.hidden("private/keep.md", false), "no un-hiding");
    assert!(!guard.hidden("docs/a.md", false));
    assert_eq!(
        guard.rules_problems(),
        [problem(
            AI_IGNORE,
            RulesWhy::BadPattern,
            RulesFallback::LastGood
        )]
    );
    // no last good rules: closed, as before
    let fresh = tempfile::tempdir().unwrap();
    let guard = Guard::checked(&root, &store_for(fresh.path(), &root));
    assert!(guard.closed());
}

/// One rule file never un-hides what another hides: a team `!pattern` leaves `.aiignore`'s folder hidden.
#[test]
fn a_team_negation_never_un_hides_what_the_ignore_file_hides() {
    let (_dir, base, root) = project();
    put(&base, AI_IGNORE, RULES.as_bytes());
    put(
        &base,
        CONFIG_FILE,
        b"[agent]\ndeny_paths = [\"!private/\", \"!private/s.md\"]\n",
    );
    let fresh = tempfile::tempdir().unwrap();
    let guard = Guard::checked(&root, &store_for(fresh.path(), &root));
    assert!(!guard.closed());
    assert!(guard.hidden("private/s.md", false));
    assert_eq!(guard.file("private/s.md"), Err(Refusal::Missing));
    assert!(!guard.hidden("docs/a.md", false));
}
