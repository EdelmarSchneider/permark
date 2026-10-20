//! Plugin `fmt-toon`: TOON (Token-Oriented Object Notation) files and `toon` blocks as a sheet of cards and tables,
//! read by one pinned version of the specification (`codec`). Edits splice the bytes they change in place (`edit`);
//! the document is never encoded again. RPC `toJson` gives the data as JSON (Copy as JSON).

mod codec;
mod edit;
mod scan;

use std::borrow::Cow;
use std::sync::Arc;

use permark_core::conditions::Requirement;
use permark_core::format::config::{ConfigEdit, EditRefusal, RowValue, Sheet, Splice};
use permark_core::format::{
    Body, Claim, Contributions, Format, FormatId, Problem, RenderCx, Rendered,
};
use permark_core::plugins::{
    typed, Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running,
};
use permark_core::{ErrorKind, IpcError};
use serde::Deserialize;

pub use codec::{decode, encode, indent_of, SPEC_LABEL, SPEC_VERSION};
pub use edit::{key_token, plain_safe, quoted};

const ID: PluginId = PluginId::of("fmt-toon");

/// Texts over this many bytes are shown as text.
pub const SHEET_MAX_BYTES: usize = 2 * 1024 * 1024;
/// A sheet with more rows and cells than this is shown as text.
pub const SHEET_MAX_ROWS: usize = 30_000;
/// Longest text handed to search.
const SEARCH_LIMIT: usize = 200_000;

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(FmtToon {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Format,
            scope: PluginScope::App,
            requires: Requirement::NONE,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

struct FmtToon {
    manifest: Manifest,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
}

impl Running for Started {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextArgs {
    text: String,
}

impl Plugin for FmtToon {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let rpc = cx
            .rpc(
                "toJson",
                typed(|args: TextArgs, _| {
                    let value = decode(&args.text).map_err(|p| {
                        IpcError::new(ErrorKind::InvalidData)
                            .detail(format!("{}: {}", p.line, p.detail))
                    })?;
                    serde_json::to_string_pretty(&value)
                        .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))
                }),
            )
            .into_iter()
            .collect();
        Ok(Box::new(Started { _rpc: rpc }))
    }

    fn contributions(&self) -> Contributions {
        Contributions::default().formats(vec![Arc::new(ToonFormat)])
    }
}

/// The TOON format: claims `*.toon`, reads a sheet, edits in place.
#[derive(Debug, Clone, Copy)]
pub struct ToonFormat;

impl ToonFormat {
    fn text_body(text: &str, note: &'static str, problem: Option<Problem>) -> Rendered {
        Rendered {
            body: Body::Text {
                text: text.to_owned(),
                language: Some("toon".to_owned()),
                problem,
            },
            notes: vec![note],
        }
    }
}

impl Format for ToonFormat {
    fn id(&self) -> FormatId {
        FormatId::TOON
    }

    fn claims(&self, _name: &str, ext: Option<&str>, _head: &[u8]) -> Claim {
        if ext == Some("toon") {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn render(&self, text: &str, _cx: &RenderCx<'_>) -> Rendered {
        if text.len() > SHEET_MAX_BYTES {
            return Self::text_body(text, "bigConfig", None);
        }
        if let Err(problem) = decode(text) {
            return Self::text_body(text, "parseError", Some(problem));
        }
        let sheet = scan::scan(text).sheet;
        if sheet.row_count() > SHEET_MAX_ROWS {
            return Self::text_body(text, "bigConfig", None);
        }
        Rendered {
            body: Body::Sheet { sheet },
            notes: Vec::new(),
        }
    }

    fn sheet(&self, text: &str) -> Option<Sheet> {
        if text.len() > SHEET_MAX_BYTES || decode(text).is_err() {
            return None;
        }
        Some(scan::scan(text).sheet)
    }

    fn edit(&self, text: &str, edit: &ConfigEdit) -> Result<Splice, EditRefusal> {
        if text.len() > SHEET_MAX_BYTES {
            return Err(EditRefusal::Unsupported);
        }
        edit::edit(text, edit)
    }

    fn searchable(&self, text: &str) -> Option<String> {
        let sheet = self.sheet(text)?;
        let mut out = sheet.keys_text(SEARCH_LIMIT / 2);
        // data values are content: they are searched, except the secret ones
        let mut push = |s: &permark_core::format::config::Scalar| {
            if !s.secret && !s.text.is_empty() && out.len() + s.text.len() < SEARCH_LIMIT {
                out.push_str(&s.text);
                out.push('\n');
            }
        };
        for (_, row) in sheet.rows() {
            match &row.value {
                RowValue::Scalar(s) => push(s),
                RowValue::List { items } => items.iter().for_each(&mut push),
                RowValue::Table(t) => t
                    .rows
                    .iter()
                    .flat_map(|r| r.cells.iter())
                    .for_each(&mut push),
                RowValue::Raw { .. } | RowValue::Pattern { .. } => {}
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests;
