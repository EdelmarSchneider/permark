//! `reviewed.json`: which version of each document the person approved, and when a document that is gone was last
//! seen to be gone.
//!
//! ```json
//! {
//!   "version": 1,
//!   "documents": {
//!     "plan.md": { "hash": "00000000000000a1" },
//!     "old/notes.md": { "hash": "00000000000000b2", "deletedAt": 1760000000000 }
//!   }
//! }
//! ```
//!
//! `hash` is the xxh3 hash of the approved text in the form `read.json` uses (16 lowercase hex digits). `deletedAt`
//! (milliseconds since the Unix epoch) is there while the file is gone: the review forgets such a document after a
//! while ([`super::book`]). A file of another `version` is not read: the review starts over from what was read.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::unread::{hash_from_hex, hash_to_hex};

/// The format this build reads and writes.
pub const VERSION: u32 = 1;

/// What is kept of one document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedDoc {
    /// The hash of the approved text.
    pub hash: u64,
    /// When the file was first seen to be gone (`None` while it is there).
    pub deleted_at: Option<u64>,
}

/// The content of `reviewed.json`, by path as the UI spells it.
pub type Saved = BTreeMap<String, SavedDoc>;

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    documents: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryOut {
    hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted_at: Option<u64>,
}

fn entry_of(value: &serde_json::Value) -> Option<SavedDoc> {
    let hash = hash_from_hex(value.get("hash")?.as_str()?)?;
    Some(SavedDoc {
        hash,
        deleted_at: value.get("deletedAt").and_then(serde_json::Value::as_u64),
    })
}

/// The documents `reviewed.json` holds; `None` when it is not a file of this version. An entry that is not a path
/// with a hash is dropped, which leaves that document without an approved version.
pub fn decode(bytes: &[u8]) -> Option<Saved> {
    let file: File = serde_json::from_slice(bytes).ok()?;
    if file.version != VERSION {
        return None;
    }
    Some(
        file.documents
            .into_iter()
            .filter_map(|(path, value)| Some((path, entry_of(&value)?)))
            .collect(),
    )
}

pub fn encode(saved: &Saved) -> Option<Vec<u8>> {
    let documents: BTreeMap<&str, EntryOut> = saved
        .iter()
        .map(|(path, doc)| {
            (
                path.as_str(),
                EntryOut {
                    hash: hash_to_hex(doc.hash),
                    deleted_at: doc.deleted_at,
                },
            )
        })
        .collect();
    let mut out = serde_json::to_vec_pretty(&serde_json::json!({
        "version": VERSION,
        "documents": documents,
    }))
    .ok()?;
    out.push(b'\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(hash: u64, deleted_at: Option<u64>) -> SavedDoc {
        SavedDoc { hash, deleted_at }
    }

    #[test]
    fn what_is_written_is_read_back() {
        let saved: Saved = BTreeMap::from([
            ("plan.md".to_owned(), doc(0xa1, None)),
            (
                "old/notes.md".to_owned(),
                doc(0xb2, Some(1_760_000_000_000)),
            ),
        ]);
        let bytes = encode(&saved).expect("encode");
        assert_eq!(decode(&bytes), Some(saved));
    }

    #[test]
    fn the_file_says_its_version_and_spells_hashes_like_read_json() {
        let saved: Saved = BTreeMap::from([("a.md".to_owned(), doc(0xff, None))]);
        let json: serde_json::Value =
            serde_json::from_slice(&encode(&saved).expect("encode")).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "version": 1,
                "documents": { "a.md": { "hash": "00000000000000ff" } }
            })
        );
    }

    #[test]
    fn a_file_of_another_version_or_form_is_not_read() {
        assert_eq!(
            decode(br#"{"a.md": "00000000000000ff"}"#),
            None,
            "no version"
        );
        assert_eq!(decode(br#"{"version": 2, "documents": {}}"#), None, "later");
        assert_eq!(
            decode(br#"{"version": 0, "documents": {}}"#),
            None,
            "earlier"
        );
        assert_eq!(decode(b"[1, 2]"), None);
        assert_eq!(decode(b"not json"), None);
        assert_eq!(decode(b""), None);
    }

    #[test]
    fn an_entry_that_is_not_a_path_with_a_hash_is_dropped_and_the_rest_is_kept() {
        let bytes = br#"{
            "version": 1,
            "documents": {
                "good.md": { "hash": "00000000000000ff" },
                "short.md": { "hash": "ff" },
                "upper.md": { "hash": "00000000000000FF", "deletedAt": 5 },
                "number.md": { "hash": 255 },
                "bare.md": "00000000000000ff",
                "dated.md": { "hash": "0000000000000001", "deletedAt": "yesterday" }
            }
        }"#;
        let saved = decode(bytes).expect("decode");
        assert_eq!(
            saved.keys().map(String::as_str).collect::<Vec<_>>(),
            ["dated.md", "good.md", "upper.md"]
        );
        assert_eq!(
            saved["dated.md"],
            doc(1, None),
            "a date that is no date is none"
        );
        assert_eq!(saved["upper.md"].deleted_at, Some(5));
    }
}
