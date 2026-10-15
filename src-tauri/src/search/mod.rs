//! Section-level full-text search: chunking, folding and an in-memory BM25 index.
//!
//! The [`SearchProvider`] trait is the seam for the v0.2 semantic layer: a hybrid provider can
//! fuse BM25 results with embedding results (reciprocal rank fusion) behind the same interface.

mod bm25;
pub(crate) mod chunk;
mod fold;
pub mod indexer;

use std::sync::RwLock;

use serde::Serialize;

pub use bm25::Bm25Index;

/// One matching section. Matches the UI's `SearchHit`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub heading_path: Vec<String>,
    /// HTML-escaped excerpt with matches wrapped in `<mark>`.
    pub snippet: String,
    pub start_line: u32,
    pub score: f32,
}

/// Something that can answer search queries.
pub trait SearchProvider: Send + Sync {
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit>;
}

/// A shared, incrementally updated index.
impl SearchProvider for RwLock<Bm25Index> {
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        self.read()
            .map(|index| index.search(query, limit))
            .unwrap_or_default()
    }
}
