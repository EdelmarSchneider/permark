//! Agent files at a project's root (`conditions.md` `agent_files`): the one list Live's default and the agent-share
//! offer read (through `Context.agent_files`), with the `agent` plugin on or off. Existence only, root only: a few
//! name checks, never a walk, never a read of the content.

use crate::paths::{Vault, VaultPath};

/// Names checked at the root, in this order; a trailing `/` = a folder. `SKILL.md` + skills live under `.claude/`.
pub const AGENT_FILES: [&str; 9] = [
    "AGENTS.md",
    "CLAUDE.md",
    "GEMINI.md",
    ".mcp.json",
    ".cursorrules",
    ".windsurfrules",
    ".claude/",
    ".cursor/",
    ".github/copilot-instructions.md",
];

/// The names of [`AGENT_FILES`] found at the root of `vault`, list order, as listed. Each through `Vault::resolve`
/// (a link | junction leading out of the folder is not counted); the kind must match (file | folder).
#[must_use]
pub fn agent_files(vault: &Vault) -> Vec<String> {
    AGENT_FILES
        .iter()
        .filter(|name| {
            let (path, folder) = match name.strip_suffix('/') {
                Some(path) => (path, true),
                None => (**name, false),
            };
            VaultPath::parse(path)
                .ok()
                .and_then(|parsed| vault.resolve(&parsed).ok())
                .and_then(|abs| std::fs::metadata(abs).ok())
                .is_some_and(|meta| {
                    if folder {
                        meta.is_dir()
                    } else {
                        meta.is_file()
                    }
                })
        })
        .map(|name| (*name).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::paths::Root;

    fn vault(dir: &Path) -> Vault {
        Vault::new(Root::open(dir).unwrap())
    }

    fn write(dir: &Path, rel: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    #[test]
    fn every_name_found_is_listed_in_list_order() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(agent_files(&vault(dir.path())), Vec::<String>::new());
        write(dir.path(), ".github/copilot-instructions.md");
        write(dir.path(), "CLAUDE.md");
        fs::create_dir_all(dir.path().join(".claude/skills/x")).unwrap();
        write(dir.path(), "AGENTS.md");
        write(dir.path(), ".mcp.json");
        assert_eq!(
            agent_files(&vault(dir.path())),
            [
                "AGENTS.md",
                "CLAUDE.md",
                ".mcp.json",
                ".claude/",
                ".github/copilot-instructions.md"
            ]
        );
        // every name at once
        for name in AGENT_FILES {
            match name.strip_suffix('/') {
                Some(folder) => fs::create_dir_all(dir.path().join(folder)).unwrap(),
                None => write(dir.path(), name),
            }
        }
        assert_eq!(agent_files(&vault(dir.path())), AGENT_FILES);
    }

    #[test]
    fn the_wrong_kind_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("AGENTS.md")).unwrap();
        write(dir.path(), ".claude");
        fs::create_dir_all(dir.path().join(".github/copilot-instructions.md")).unwrap();
        assert_eq!(agent_files(&vault(dir.path())), Vec::<String>::new());
    }

    #[test]
    fn only_the_root_counts_a_nested_project_counts_for_itself() {
        let dir = tempfile::tempdir().unwrap();
        // a nested project with its own repository and agent files, and a big tree of them below: never a walk
        fs::create_dir_all(dir.path().join("hub b/.git")).unwrap();
        write(dir.path(), "hub b/AGENTS.md");
        write(dir.path(), "hub b/.claude/CLAUDE.md");
        for n in 0..2000 {
            write(dir.path(), &format!("deep/d{n:04}/AGENTS.md"));
        }
        write(dir.path(), "docs/CLAUDE.md");
        assert_eq!(agent_files(&vault(dir.path())), Vec::<String>::new());
        // the project opened alone: its own root
        assert_eq!(
            agent_files(&vault(&dir.path().join("hub b"))),
            ["AGENTS.md", ".claude/"]
        );
    }

    #[test]
    fn case_follows_the_file_system() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "agents.md");
        write(dir.path(), "ünïcode/CLAUDE.md");
        let ignores_case = dir.path().join("AGENTS.md").exists();
        let expected: &[&str] = if ignores_case { &["AGENTS.md"] } else { &[] };
        assert_eq!(agent_files(&vault(dir.path())), expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_counts_a_link_leading_out_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(dir.path(), "docs/real.md");
        write(outside.path(), "AGENTS.md");
        fs::create_dir_all(outside.path().join("rules")).unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("docs/real.md"),
            dir.path().join("CLAUDE.md"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("AGENTS.md"),
            dir.path().join("AGENTS.md"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path().join("rules"), dir.path().join(".cursor"))
            .unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing.md"), dir.path().join("GEMINI.md"))
            .unwrap();
        assert_eq!(agent_files(&vault(dir.path())), ["CLAUDE.md"]);
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_inside_counts_a_junction_leading_out_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("rules")).unwrap();
        fs::create_dir_all(outside.path().join("rules")).unwrap();
        if !junction(&dir.path().join(".claude"), &dir.path().join("rules"))
            || !junction(&dir.path().join(".cursor"), &outside.path().join("rules"))
        {
            eprintln!("SKIPPED: cannot make a junction here");
            return;
        }
        assert_eq!(agent_files(&vault(dir.path())), [".claude/"]);
    }
}
