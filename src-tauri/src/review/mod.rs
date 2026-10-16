//! Review changes: what an agent changed in a document since the person last read it, and the means to keep
//! or take back each part.

// The pieces are built one by one and wired to the workspace at the end.
#![allow(dead_code)]

pub mod book;
pub mod committed;
pub mod diff;
pub mod store;
pub mod view;
