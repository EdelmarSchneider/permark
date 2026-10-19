//! What the watch tells its subscribers.

/// What happened to a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Modified,
    Removed,
    /// Moved here from `from` (project paths).
    Renamed {
        from: String,
    },
}

/// One verified change (the disk was looked at).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Project path (`@wt/<name>/...` in a worktree).
    pub path: String,
    pub kind: ChangeKind,
    /// A folder (for `Removed`: it was one when last seen).
    pub is_dir: bool,
    /// The app's own doing (a save, a move, a delete): the tree and the index learn from it, a feed ignores it.
    pub own: bool,
}

/// Changes of one flush, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FsBatch {
    pub changes: Vec<Change>,
    /// Many changes at once (more than [`BULK_CHANGES`], a git operation, an expansion cut off, an overflow).
    pub bulk: bool,
    /// Events were lost (overflow): consumers that keep a picture of the disk rebuild it.
    pub rescan: bool,
}

impl FsBatch {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && !self.rescan
    }
}

/// More changes than this in one flush make a bulk batch.
pub const BULK_CHANGES: usize = 200;
