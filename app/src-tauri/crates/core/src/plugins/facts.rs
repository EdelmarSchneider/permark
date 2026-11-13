//! Facts a plugin writes for the rest of the app to read: the git and agent conditions of a project (windows read them
//! into their `Context`) and the read-only reader of the repository other plugins use.

use std::sync::{Arc, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::conditions::{AgentCondition, GitCondition};
use crate::watch::StatusHook;

/// Hex digits of a SHA-1 commit id.
const SHA1_HEX: usize = 40;
/// Hex digits of a SHA-256 commit id.
const SHA256_HEX: usize = 64;

/// A full commit id: 40 (SHA-1) | 64 (SHA-256) hex digits, kept lowercase. Parse, don't validate: a commit named by
/// a caller reaches git only as this type, so it is never a ref name, an option, a short id or a revision expression.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommitId {
    /// The digits; the bytes past `len` stay zero (equality and hashing read the whole buffer).
    hex: [u8; SHA256_HEX],
    len: u8,
}

impl CommitId {
    /// `text` as a commit id; `None`: anything but 40 | 64 hex digits.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.as_bytes();
        let len = match digits.len() {
            SHA1_HEX | SHA256_HEX => u8::try_from(digits.len()).ok()?,
            _ => return None,
        };
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        let mut hex = [0_u8; SHA256_HEX];
        for (slot, digit) in hex.iter_mut().zip(digits) {
            *slot = digit.to_ascii_lowercase();
        }
        Some(Self { hex, len })
    }

    /// The lowercase hex digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // ASCII hex digits by construction
        std::str::from_utf8(&self.hex[..usize::from(self.len)]).unwrap_or_default()
    }
}

impl std::fmt::Display for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CommitId({})", self.as_str())
    }
}

impl Serialize for CommitId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// An invalid id never deserializes.
impl<'de> Deserialize<'de> for CommitId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("not a full commit id"))
    }
}

/// In TypeScript a commit id is a plain `string`: no declaration, no file of its own.
impl TS for CommitId {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }

    fn inline(cfg: &ts_rs::Config) -> String {
        <Self as TS>::name(cfg)
    }
}

/// Why [`RepoReader::blob_at`] has no bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NoBlob {
    /// No repository, no git to ask, or the call failed.
    #[error("the repository cannot be read")]
    Unavailable,
    /// No such commit, no such file at that commit, or the object is not on this computer (a partial copy of the
    /// repository: nothing is fetched).
    #[error("the file is not there at that commit")]
    Missing,
    /// The file as committed is larger than the reader hands out.
    #[error("the file at that commit is too large")]
    TooLarge,
}

/// Read-only look at the repository of a project (set by the `git` plugin while it runs).
pub trait RepoReader: Send + Sync {
    /// The file at HEAD (`path` is a project path); `None`: no repository, not committed, unreadable.
    fn blob_at_head(&self, path: &str) -> Option<Vec<u8>>;
    /// The file at `commit` (`path` is a project path), the bytes as committed. Reads what is on this computer only.
    ///
    /// # Errors
    /// [`NoBlob`] says why there are no bytes (a reader that knows no history: `Unavailable`).
    fn blob_at(&self, _commit: CommitId, _path: &str) -> Result<Vec<u8>, NoBlob> {
        Err(NoBlob::Unavailable)
    }
    /// The commit HEAD points at; `None`: no repository | no commit yet.
    fn head(&self) -> Option<String> {
        None
    }
    /// Commits from `old` to `new` (`old..new`); `None`: unknown.
    fn commits_between(&self, _old: &str, _new: &str) -> Option<u32> {
        None
    }
}

/// The [`RepoReader`] as a service (`Services::get::<RepoHandle>()`).
#[derive(Clone)]
pub struct RepoHandle(pub Arc<dyn RepoReader>);

impl std::fmt::Debug for RepoHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoHandle").finish_non_exhaustive()
    }
}

/// What plugins publish about a project. A change calls the status hook, which recomputes the windows' contexts.
pub struct ProjectFacts {
    git: RwLock<GitCondition>,
    agent: RwLock<AgentCondition>,
    hook: Option<StatusHook>,
}

impl std::fmt::Debug for ProjectFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectFacts")
            .field("git", &self.git())
            .finish_non_exhaustive()
    }
}

impl ProjectFacts {
    #[must_use]
    pub fn new(hook: Option<StatusHook>) -> Arc<Self> {
        Arc::new(Self {
            git: RwLock::new(GitCondition::NoRepo),
            agent: RwLock::new(AgentCondition::Off),
            hook,
        })
    }

    #[must_use]
    pub fn git(&self) -> GitCondition {
        self.git
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn agent(&self) -> AgentCondition {
        self.agent
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sets the agent condition (written by the `agent` plugin); announces it only when it differs.
    pub fn set_agent(&self, condition: AgentCondition) {
        {
            let mut agent = self.agent.write().unwrap_or_else(PoisonError::into_inner);
            if *agent == condition {
                return;
            }
            *agent = condition;
        }
        if let Some(hook) = &self.hook {
            hook();
        }
    }

    /// Sets the git condition; announces it only when it differs.
    pub fn set_git(&self, condition: GitCondition) {
        {
            let mut git = self.git.write().unwrap_or_else(PoisonError::into_inner);
            if *git == condition {
                return;
            }
            *git = condition;
        }
        if let Some(hook) = &self.hook {
            hook();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::conditions::RepoState;

    #[test]
    fn a_commit_id_is_a_full_hex_id_kept_lowercase() {
        let sha1 = "0123456789abcdef0123456789ABCDEF01234567";
        let id = CommitId::parse(sha1).unwrap();
        assert_eq!(id.as_str(), sha1.to_lowercase());
        assert_eq!(id.to_string(), sha1.to_lowercase());
        assert_eq!(
            format!("{id:?}"),
            format!("CommitId({})", sha1.to_lowercase())
        );
        let sha256 = CommitId::parse(&"AB".repeat(32)).unwrap();
        assert_eq!(sha256.as_str(), "ab".repeat(32));
        // the same id in another case is the same id (also as a key)
        let copy = id;
        let known = HashSet::from([id, sha256]);
        assert_eq!(CommitId::parse(&sha1.to_uppercase()), Some(copy));
        assert!(known.contains(&CommitId::parse(&sha1.to_uppercase()).unwrap()));
        assert_ne!(id, sha256);
    }

    #[test]
    fn nothing_but_a_full_hex_id_is_a_commit_id() {
        let a = |n: usize| "a".repeat(n);
        let bad = [
            String::new(),
            "HEAD".to_owned(),
            "-x".to_owned(),
            "abc1234".to_owned(),
            a(39),
            a(41),
            a(63),
            a(65),
            format!("{}g", a(39)),
            format!("-{}", a(39)),
            format!("-{}", a(40)),
            format!(" {}", a(39)),
            format!("{} ", a(39)),
            format!("{} ", a(40)),
            format!("{}\n", a(40)),
            format!("{}\0", a(39)),
            format!("{}\0{}", a(20), a(19)),
            format!("{}^", a(39)),
            format!("{}:{}", a(20), a(19)),
            // 40 bytes, 20 letters
            "é".repeat(20),
        ];
        for text in bad {
            assert_eq!(CommitId::parse(&text), None, "{text:?}");
        }
    }

    #[test]
    fn a_commit_id_travels_as_its_hex_text_and_is_checked_on_the_way_in() {
        let id = CommitId::parse(&"AB".repeat(20)).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", "ab".repeat(20)));
        assert_eq!(serde_json::from_str::<CommitId>(&json).unwrap(), id);
        assert_eq!(
            serde_json::from_value::<CommitId>(serde_json::Value::String("CD".repeat(32))).unwrap(),
            CommitId::parse(&"cd".repeat(32)).unwrap()
        );
        for bad in [
            "\"HEAD\"",
            "\"abc1234\"",
            "\"--upload-pack=x\"",
            "\"\"",
            "42",
            "null",
        ] {
            assert!(serde_json::from_str::<CommitId>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_commit_id_is_a_plain_string_in_typescript() {
        #[derive(TS)]
        #[allow(dead_code)]
        struct Row {
            id: CommitId,
        }
        #[derive(TS)]
        #[allow(dead_code)]
        enum Pick {
            Head,
            Version(CommitId),
        }
        let cfg = ts_rs::Config::default();
        assert_eq!(<CommitId as TS>::name(&cfg), "string");
        assert!(
            Row::inline(&cfg).contains("id: string"),
            "{}",
            Row::inline(&cfg)
        );
        assert!(
            Pick::inline(&cfg).contains("\"Version\": string"),
            "{}",
            Pick::inline(&cfg)
        );
        // no declaration of its own: nothing to import, no file written
        assert_eq!(Row::dependencies(&cfg).len(), 0);
        assert!(CommitId::output_path().is_none());
    }

    #[test]
    fn a_reader_that_reads_head_only_has_no_bytes_at_a_commit() {
        struct HeadOnly;
        impl RepoReader for HeadOnly {
            fn blob_at_head(&self, _: &str) -> Option<Vec<u8>> {
                Some(b"head".to_vec())
            }
        }
        let id = CommitId::parse(&"a".repeat(40)).unwrap();
        assert_eq!(HeadOnly.blob_at(id, "a.md"), Err(NoBlob::Unavailable));
    }

    #[test]
    fn the_hook_hears_only_changes() {
        let heard = Arc::new(AtomicUsize::new(0));
        let hook: StatusHook = {
            let heard = Arc::clone(&heard);
            Arc::new(move || {
                heard.fetch_add(1, Ordering::SeqCst);
            })
        };
        let facts = ProjectFacts::new(Some(hook));
        assert_eq!(facts.git(), GitCondition::NoRepo);
        facts.set_git(GitCondition::NoRepo);
        assert_eq!(heard.load(Ordering::SeqCst), 0);
        facts.set_git(GitCondition::Repo {
            state: RepoState::Clean,
        });
        facts.set_git(GitCondition::Repo {
            state: RepoState::Clean,
        });
        facts.set_git(GitCondition::Repo {
            state: RepoState::Merging,
        });
        assert_eq!(heard.load(Ordering::SeqCst), 2);
    }
}
