//! Facts a plugin writes for the rest of the app to read: the git and agent conditions of a project (windows read them
//! into their `Context`) and the read-only reader of the repository other plugins use.

use std::sync::{Arc, PoisonError, RwLock};

use crate::conditions::{AgentCondition, GitCondition};
use crate::watch::StatusHook;

/// Read-only look at the repository of a project (set by the `git` plugin while it runs).
pub trait RepoReader: Send + Sync {
    /// The file at HEAD (`path` is a project path); `None`: no repository, not committed, unreadable.
    fn blob_at_head(&self, path: &str) -> Option<Vec<u8>>;
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::conditions::RepoState;

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
