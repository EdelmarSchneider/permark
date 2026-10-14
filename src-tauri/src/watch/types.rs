//! Payload types of the `fs-change` event (they mirror `FsBatch` in the UI contract).

use serde::Serialize;

/// Bulk batches list at most this many paths.
pub const BULK_PATH_CAP: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Created,
    Modified,
    Removed,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChange {
    pub kind: ChangeKind,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BulkReason {
    GitOperation,
    Large,
    Overflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bulk {
    pub count: usize,
    pub reason: BulkReason,
    /// At most [`BULK_PATH_CAP`] paths.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsBatch {
    pub changes: Vec<FsChange>,
    pub bulk: Option<Bulk>,
    /// Events were lost: the UI should refresh everything.
    pub rescan: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_the_ui_contract() {
        let batch = FsBatch {
            changes: vec![
                FsChange {
                    kind: ChangeKind::Renamed,
                    path: "b.md".to_owned(),
                    from: Some("a.md".to_owned()),
                    at_ms: 5,
                },
                FsChange {
                    kind: ChangeKind::Modified,
                    path: "c.md".to_owned(),
                    from: None,
                    at_ms: 6,
                },
            ],
            bulk: Some(Bulk {
                count: 143,
                reason: BulkReason::GitOperation,
                paths: vec!["x.md".to_owned()],
            }),
            rescan: false,
        };
        let json = serde_json::to_value(&batch).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "changes": [
                    {"kind": "renamed", "path": "b.md", "from": "a.md", "atMs": 5},
                    {"kind": "modified", "path": "c.md", "atMs": 6},
                ],
                "bulk": {"count": 143, "reason": "gitOperation", "paths": ["x.md"]},
                "rescan": false,
            })
        );
    }
}
