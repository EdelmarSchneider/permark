//! Reading a file for the index: bytes -> sections with counted terms + the link targets. Pure apart from the read;
//! runs on the build threads.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use super::bm25::SectionInput;
use super::fold::tokenize;
use crate::format::{sections, Section};
use crate::fsx::read_text;
use crate::links::{local_target, parse as parse_links};
use crate::tree::{key_of, Category};

/// Markdown documents larger than this are listed by name only (the reader's limit).
pub const MARKDOWN_LIMIT: u64 = 5 * 1024 * 1024;
/// Plain text files larger than this are listed by name only.
pub const TEXT_LIMIT: u64 = 1024 * 1024;

/// Weights of the fields in the term frequency.
const W_OWN_HEADING: u32 = 2;
const W_ANCESTOR_HEADING: u32 = 1;
const W_BODY: u32 = 1;
const W_FILE_NAME: u32 = 3;

/// What tells a file changed without reading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sig {
    pub len: u64,
    pub mtime_ms: u64,
}

impl Sig {
    #[must_use]
    pub fn of(meta: &std::fs::Metadata) -> Self {
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        Self {
            len: meta.len(),
            mtime_ms,
        }
    }
}

/// Whether the index reads the content of this category.
#[must_use]
pub fn reads_content(category: Category) -> bool {
    matches!(category, Category::Markdown | Category::Text)
}

/// A file read and ready to insert.
#[derive(Debug)]
pub struct Prepared {
    pub path: Arc<str>,
    pub sig: Sig,
    pub sections: Vec<SectionInput>,
    /// Keys of the project paths the document links to (each once).
    pub links: Vec<String>,
}

/// File name without folder and extension: `docs/retry-policy.md` -> `retry-policy`.
fn stem_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or(name, |(stem, _)| stem)
}

fn count_into(text: &str, weight: u32, terms: &mut HashMap<String, u32>, len: &mut u32) {
    for token in tokenize(text) {
        *terms.entry(token.term).or_default() += weight;
        *len += weight;
    }
}

fn section_input(section: Section, file_name: Option<&str>) -> SectionInput {
    let mut terms: HashMap<String, u32> = HashMap::new();
    let mut len = 0_u32;
    let own = section.heading_path.len().saturating_sub(1);
    for (at, heading) in section.heading_path.iter().enumerate() {
        let weight = if at == own {
            W_OWN_HEADING
        } else {
            W_ANCESTOR_HEADING
        };
        count_into(heading, weight, &mut terms, &mut len);
    }
    count_into(&section.text, W_BODY, &mut terms, &mut len);
    if let Some(name) = file_name {
        count_into(name, W_FILE_NAME, &mut terms, &mut len);
    }
    SectionInput {
        heading_path: section.heading_path,
        anchor: section.anchor,
        line: section.line,
        text: section.text,
        terms: terms.into_iter().collect(),
        len,
    }
}

/// Reads `abs` (project path `path`) and prepares it. `None`: not readable as text, over the limit, binary.
#[must_use]
pub fn read_and_prepare(path: &str, abs: &Path, category: Category) -> Option<Prepared> {
    let limit = match category {
        Category::Markdown => MARKDOWN_LIMIT,
        Category::Text => TEXT_LIMIT,
        _ => return None,
    };
    let meta = std::fs::metadata(abs).ok()?;
    if !meta.is_file() || meta.len() > limit {
        return None;
    }
    let sig = Sig::of(&meta);
    let decoded = read_text(abs, limit).ok()?;
    Some(prepare(path, sig, &decoded.text, category))
}

/// Sections + links of a decoded text.
#[must_use]
pub fn prepare(path: &str, sig: Sig, text: &str, category: Category) -> Prepared {
    let found = if category == Category::Markdown {
        sections(text)
    } else {
        let body = text.trim();
        if body.is_empty() {
            Vec::new()
        } else {
            vec![Section {
                heading_path: Vec::new(),
                anchor: None,
                line: 1,
                text: body.to_owned(),
            }]
        }
    };
    let mut inputs: Vec<SectionInput> = Vec::with_capacity(found.len().max(1));
    let name = stem_of(path);
    let mut found = found.into_iter();
    // the file name counts once, in the document's first section (an empty document still has its name)
    match found.next() {
        Some(first) => inputs.push(section_input(first, Some(name))),
        None => inputs.push(section_input(
            Section {
                heading_path: Vec::new(),
                anchor: None,
                line: 1,
                text: String::new(),
            },
            Some(name),
        )),
    }
    inputs.extend(found.map(|s| section_input(s, None)));

    let links = if category == Category::Markdown {
        let keys: BTreeSet<String> = parse_links(text)
            .iter()
            .filter_map(|dest| local_target(path, dest.inner(text)))
            .map(|target| key_of(&target))
            .collect();
        keys.into_iter().collect()
    } else {
        Vec::new()
    };
    Prepared {
        path: Arc::from(path),
        sig,
        sections: inputs,
        links,
    }
}
