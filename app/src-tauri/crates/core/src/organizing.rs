//! What the organizing plugins (tags, silence, comments) hand to core consumers: search today, the agent connection
//! later. Typed, data-only, offered through [`crate::plugins::Services`] while the plugin runs (RAII lease); a
//! consumer asks `services.get::<TagsApi>()` and works without when there is none.

use std::collections::HashSet;
use std::sync::Arc;

/// What search and the agent ask of the tags.
pub trait TagLookup: Send + Sync {
    /// Project paths of the files that have EVERY one of `tags` (front matter and app tags together).
    fn files_with_all(&self, tags: &[String]) -> HashSet<String>;
    /// The tags of one file, front matter first.
    fn tags_of(&self, path: &str) -> Vec<String>;
}

/// The tags service of a project.
#[derive(Clone)]
pub struct TagsApi(pub Arc<dyn TagLookup>);

/// Which paths the person silenced.
pub trait SilenceLookup: Send + Sync {
    fn is_silenced(&self, path: &str) -> bool;
}

/// The silence service of a project. Personal: the agent connection never asks it.
#[derive(Clone)]
pub struct SilenceApi(pub Arc<dyn SilenceLookup>);
